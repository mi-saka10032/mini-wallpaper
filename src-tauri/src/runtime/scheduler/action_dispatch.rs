//! 用户动作派发（ActionDispatch）
//!
//! 把 [`crate::runtime::action::Action`] 翻译为对具体 `monitor_config` 的
//! 副作用调用。承担"动作语义解析"与"目标显示器解析"两件事，
//! 真正的切换 / 暂停 / 收藏等原子能力由调度器其它 impl 块提供
//! （如 `switch_to_adjacent_wallpaper` / `manage_carousel_timer`），
//! 本文件**零业务重复**。
//!
//! 所有方法通过 `impl Scheduler` 跨文件实现，与
//! `setting_effects` / `deletion_effects` / `task_lifecycle` 同级，
//! 天然拥有 `self.app`、`self.tasks` 等内部状态，零参数透传。
//!
//! ## 调用拓扑
//!
//! ```text
//!  trigger_action / global_shortcut / tray_menu
//!                       │
//!                       ▼
//!         Scheduler::dispatch_action(Action)
//!                       │
//!     ┌─────────────────┴────────────────┐
//!     ▼                                  ▼
//!  resolve_action_targets()        switch_to_adjacent_wallpaper /
//!  （display_mode + active 探测）   manage_carousel_timer / ...
//! ```
//!
//! ## 目标显示器解析规则
//!
//! - **independent**：仅作用于"用户当前活跃显示器"对应的 active config
//!   - 通过 [`monitor_geometry::active_monitor()`] 探测 device_name
//!   - 与 `monitor_configs.monitor_id` 字符串相等比较命中
//!   - 命中失败时静默放弃 + warn（不广播，避免误触发其它屏幕）
//! - **mirror / extend**：作用于第一个 active config（语义同
//!   `start_all_carousel_timers`，避免重复切换造成的双倍跳跃）

use log::{info, warn};

use super::Scheduler;
use crate::dto::shortcut_dto::Direction;
use crate::entities::monitor_config;
use crate::events::{ActionToastPayload, TypedEmit};
use crate::runtime::action::Action;
use crate::services::monitor_config_service;

// `platform::windows` 仅在 windows target 编译，因此这里通过本地 cfg 包装
// 把"探测 active 显示器 device_name"抽象为跨平台可调用的小函数。
// 非 windows target 下（CI 编译 / 跨平台开发体验）固定返回 None，
// dispatcher 会自然落入"目标为空 → warn 跳过"分支。
#[cfg(target_os = "windows")]
fn probe_active_device_name() -> Option<String> {
    crate::platform::windows::monitor_geometry::active_monitor()
        .map(|m| m.device_name)
}

#[cfg(not(target_os = "windows"))]
fn probe_active_device_name() -> Option<String> {
    None
}

impl Scheduler {
    // ==================== 公共入口（供 Command / 快捷键 / 托盘 调用） ====================

    /// 派发用户动作
    ///
    /// 单一入口翻译 [`Action`] 为对调度器既有原子能力的调用。
    /// 该方法吞掉所有非致命错误（仅 warn），不向上传播 —— 用户动作类
    /// 操作即便失败也不应阻塞 UI 或导致 Tauri 命令报错。
    pub async fn dispatch_action(&mut self, action: Action) {
        let toast_message = match &action {
            Action::Next => Some("已切换下一张壁纸"),
            Action::Prev => Some("已切换上一张壁纸"),
            Action::TogglePause => None, // TogglePause 的 toast 在内部根据状态动态生成
            Action::OpenMain => None,    // 打开窗口无需 toast
            Action::Quit => None,        // 退出无需 toast
        };

        match action {
            Action::Next => self.dispatch_switch(Direction::Next).await,
            Action::Prev => self.dispatch_switch(Direction::Prev).await,
            Action::TogglePause => self.dispatch_toggle_pause().await,
            Action::OpenMain => self.dispatch_open_main(),
            Action::Quit => self.dispatch_quit(),
        }

        // 发送 toast 反馈（仅对需要反馈的动作）
        if let Some(msg) = toast_message {
            self.emit_action_toast(&action, msg);
        }
    }

    // ==================== 动作实现 ====================

    /// 切换壁纸（Next / Prev 共用实现）
    ///
    /// 解析目标 config 集合后，逐一委托
    /// [`Scheduler::switch_to_adjacent_wallpaper`] 完成实际切换、
    /// DB 更新、窗口通知与定时器重置。
    async fn dispatch_switch(&mut self, direction: Direction) {
        let targets = match self.resolve_action_targets().await {
            Some(t) if !t.is_empty() => t,
            _ => {
                warn!(
                    "[ActionDispatch] 找不到合适的目标显示器，跳过 {:?} 动作",
                    direction
                );
                return;
            }
        };

        info!(
            "[ActionDispatch] 派发 {:?} 动作到 {} 个目标显示器",
            direction,
            targets.len()
        );

        for cfg in &targets {
            self.switch_to_adjacent_wallpaper(cfg, &direction).await;
        }
    }

    /// 切换暂停 / 恢复轮播
    ///
    /// 实现思路：
    /// - 若 active 目标当前**已暂停** → 从 `paused_monitors` 移除，
    ///   通过 `manage_carousel_timer` 走常规编排恢复定时器
    /// - 若 active 目标当前**未暂停** → 加入 `paused_monitors`，
    ///   `manage_carousel_timer` 内部会因暂停集合命中而 stop 定时器
    ///
    /// 同步模式下只对"primary active config"操作；其它显示器跟随广播
    /// 由 `manage_carousel_timer` 内部统一处理。
    async fn dispatch_toggle_pause(&mut self) {
        let targets = match self.resolve_action_targets().await {
            Some(t) if !t.is_empty() => t,
            _ => {
                warn!("[ActionDispatch] 找不到合适的目标显示器，跳过 TogglePause 动作");
                return;
            }
        };

        let mut paused = false;
        for cfg in &targets {
            let mid = cfg.monitor_id.clone();
            let was_paused = self.paused_monitors.contains(&mid);

            if was_paused {
                self.paused_monitors.remove(&mid);
                info!("[ActionDispatch] 恢复轮播: {}", mid);
            } else {
                self.paused_monitors.insert(mid.clone());
                info!("[ActionDispatch] 暂停轮播: {}", mid);
                paused = true;
            }

            // 复用既有编排：暂停集合状态变化后让 manage_carousel_timer 自动
            // 决定 stop / spawn（need_restart=false，避免重置间隔）
            self.manage_carousel_timer(cfg, false).await;
        }

        // 根据最终状态发送 toast
        let msg = if paused { "已暂停轮播" } else { "已恢复轮播" };
        self.emit_action_toast(&Action::TogglePause, msg);
    }

    /// 显示并聚焦主窗口
    ///
    /// 直接复用 `platform::tray::show_main_window` 的窗口操作逻辑，
    /// 三方调用方（托盘单击 / 托盘菜单 / Action 派发）共享同一实现。
    fn dispatch_open_main(&self) {
        crate::platform::tray::show_main_window(&self.app);
        info!("[ActionDispatch] 显示主窗口");
    }

    /// 退出应用
    ///
    /// 调用 `AppHandle::exit(0)`，Tauri 会触发 `ExitRequested` 事件，
    /// 由 `lib.rs` 中既有的退出钩子统一停止后台任务、清理资源。
    fn dispatch_quit(&self) {
        info!("[ActionDispatch] 退出应用");
        self.app.exit(0);
    }

    // ==================== Toast 反馈 ====================

    /// 向主窗口发送动作反馈 toast 事件
    fn emit_action_toast(&self, action: &Action, message: &str) {
        let action_name = match action {
            Action::Next => "next",
            Action::Prev => "prev",
            Action::TogglePause => "togglePause",
            Action::OpenMain => "openMain",
            Action::Quit => "quit",
        };
        let payload = ActionToastPayload {
            action: action_name.to_string(),
            message: message.to_string(),
        };
        if let Err(e) = self.app.typed_emit(&payload) {
            warn!("[ActionDispatch] 发送 toast 事件失败: {}", e);
        }
    }

    // ==================== 目标显示器解析 ====================

    /// 解析当前动作应作用的目标 monitor_config 集合
    ///
    /// 解析策略：
    /// - **同步模式（mirror/extend）**：返回第一个 active config 包成单元素 Vec，
    ///   切换函数内部会通过 `notify_wallpaper_update(is_sync=true)` 广播到其它屏
    /// - **独立模式（independent）**：通过 Win32 探测 active 显示器的 device_name，
    ///   与 active config 列表精确匹配；命中失败返回 `Some(vec![])` 让上层 warn 跳过
    ///
    /// 返回 `None` 仅在 DB 查询失败时出现（极少数异常路径）。
    async fn resolve_action_targets(&self) -> Option<Vec<monitor_config::Model>> {
        let db = self.db();

        let configs = match monitor_config_service::get_all(&db).await {
            Ok(c) => c,
            Err(e) => {
                warn!("[ActionDispatch] 查询 monitor_configs 失败: {}", e);
                return None;
            }
        };

        // 仅考虑 active 配置，无 active 直接返回空目标
        let active_configs: Vec<&monitor_config::Model> =
            configs.iter().filter(|c| c.active).collect();
        if active_configs.is_empty() {
            return Some(Vec::new());
        }

        // 同步模式：取第一个 active config，由切换函数内部广播
        if self.resolve_is_sync_mode().await {
            return Some(vec![active_configs[0].clone()]);
        }

        // 独立模式：精确匹配 active 显示器的 device_name
        let active_name = match probe_active_device_name() {
            Some(name) => name,
            None => {
                warn!("[ActionDispatch] active_monitor 探测失败");
                return Some(Vec::new());
            }
        };

        let hit = active_configs
            .iter()
            .find(|c| c.monitor_id == active_name)
            .map(|c| (*c).clone());

        match hit {
            Some(cfg) => Some(vec![cfg]),
            None => {
                warn!(
                    "[ActionDispatch] active 显示器 '{}' 在 active configs 中未命中",
                    active_name
                );
                Some(Vec::new())
            }
        }
    }
}