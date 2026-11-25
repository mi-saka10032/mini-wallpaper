//! 统一壁纸管理器（架构重整 Phase 2）
//!
//! 围绕 `monitor_id` 统一管理 Native surface 与 WebView 窗口的匹配关系，
//! 是后续 action → reconcile 派发的唯一入口。
//!
//! 本阶段采用**组合复用**而非物理合并：`RenderCoordinator` 与
//! `WallpaperWindowManager` 的既有逻辑保持不动，由 Manager 在 init 阶段
//! 统一做显示器枚举、几何登记与首帧建窗。这样既能让 Rust 侧成为建窗发起方，
//! 又避免一次性搬动 1200+ 行 Windows-only 渲染代码带来的不可验证风险；
//! 待 Phase 3 把散落的调用点全部收编后，再收口两者边界。

// init 的建窗主体是 Windows-only 分支，且 reconcile 的对外查询方法要到 Phase 3
// 才被调用；非 Windows 目标与当前阶段都会把它们判为未使用。Phase 3 接线后移除本行。
#![allow(dead_code)]

use std::collections::HashMap;

use anyhow::Result;
use sea_orm::DatabaseConnection;
use tauri::AppHandle;

use crate::platform::wallpaper_renderer::{
    default_backend_selection, BackendSelection, DesktopRenderer, DisplayMode, MonitorRect,
    RenderBackend,
};

use crate::ctx::unified_manager::engine::{
    ConfigSnapshot, DispatchAction, GlobalSnapshot, MonitorSnapshot, DEFAULT_DISPLAY_MODE,
};
pub mod engine;
pub mod errors;
pub mod monitors;
pub mod native;
pub mod reconcile;
pub mod webview;

// 外部只需 ReconcileScope 作为 reconcile() 的范围参数，经 manager 门面暴露，
// 调用方不必依赖 engine 内部路径。
pub use engine::ReconcileScope;

use crate::dto::app_setting_dto::keys as setting_keys;
use crate::events::{ExtendViewportPayload, TypedEmitTo};
use crate::platform::wallpaper_renderer::group_viewports;
use crate::services::{app_setting_service, monitor_config_service};

/// 单个物理显示器在 Manager 内的登记项。
///
/// `rect` 恒为物理像素虚拟桌面坐标；`dpi` 来自目标屏自身，用于 WebView
/// 建窗时把物理像素换算成 Tauri builder 的逻辑像素（M18）。
#[derive(Debug, Clone)]
pub struct MonitorSlot {
    pub rect: MonitorRect,
    pub dpi: u32,
    pub has_target: bool,
}

/// 把 Win32 的 `MONITORINFOEXW` 矩形转成渲染契约使用的 x/y/width/height。
/// 两套 MonitorRect 同名但语义不同，转换集中在此处，避免散落各处手写。
#[cfg(target_os = "windows")]
fn to_render_rect(
    info: &crate::platform::windows::monitor_geometry::MonitorInfo,
) -> (String, MonitorRect, u32) {
    (
        info.rect.device_name.clone(),
        MonitorRect {
            x: info.rect.left,
            y: info.rect.top,
            width: info.rect.width(),
            height: info.rect.height(),
        },
        info.dpi,
    )
}

/// 从 DB 的 monitor_configs + app_setting 白名单构建初始快照。
/// 纯函数，便于单测；字段范围与 `ctx::reconcile` 保持一致（仅窗口相关三项）。
pub fn build_snapshot(
    configs: &[crate::entities::monitor_config::Model],
    display_mode: String,
    volume: u32,
) -> ConfigSnapshot {
    let monitors = configs
        .iter()
        .map(|config| {
            (
                config.monitor_id.clone(),
                MonitorSnapshot {
                    wallpaper_id: config.wallpaper_id,
                    fit_mode: config.fit_mode.clone(),
                    active: config.active,
                },
            )
        })
        .collect();
    ConfigSnapshot {
        monitors,
        global: GlobalSnapshot {
            display_mode,
            volume,
        },
    }
}

pub(super) fn action_monitor_id(action: &DispatchAction) -> Option<&str> {
    match action {
        DispatchAction::EnsureTarget { monitor_id, .. }
        | DispatchAction::DestroyTarget { monitor_id }
        | DispatchAction::UpdateWallpaper { monitor_id, .. }
        | DispatchAction::ClearWallpaper { monitor_id }
        | DispatchAction::SetFitMode { monitor_id, .. } => Some(monitor_id),
        DispatchAction::SetDisplayMode { .. } | DispatchAction::SetVolume { .. } => None,
    }
}

/// 统一壁纸管理器：App 的唯一主体 manager。
///
/// 由原 RenderCoordinator / WallpaperWindowManager 拉平而来：字段全部并入本 struct，
/// native（渲染链路）与 webview（窗口链路）的 impl 分置 `native.rs` / `webview.rs`。
/// backend 路由决策与 reconcile 编排留在本文件（主体），两个 impl 只负责"怎么执行"。
pub struct UnifiedWallpaperManager {
    // ===== 基础设施（原三 struct 重复持有，合并为一份） =====
    db: DatabaseConnection,
    /// 用于按 monitor_id 定向推送事件、广播到窗口（原 UM / RC / WM 三处持有）。
    app_handle: AppHandle,

    // ===== 主体状态（原 UM） =====
    slots: HashMap<String, MonitorSlot>,
    /// 已成功应用的窗口状态，init 完成后作为后续 reconcile 的起点。
    observed: ConfigSnapshot,

    // ===== Native 渲染链路状态（原 RC，字段原样搬运） =====
    renderer: DesktopRenderer,
    /// backend 路由策略（Auto / NativeOnly）。
    selection: BackendSelection,
    /// monitor_id -> 当前 active backend；apply_backend 会就地改写，
    /// 跨 backend 判断必须提前快照。
    backends: HashMap<String, RenderBackend>,
    /// monitor_id -> 当前壁纸 id；判定换图是否触发 crossfade。
    wallpaper_ids: HashMap<String, i32>,
    /// monitor_id -> 显示器几何（含无壁纸屏，保证 extend union 不缺块）。
    rects: HashMap<String, MonitorRect>,
    display_mode: DisplayMode,
    /// 降级提示只发一次，恢复为全 Native 时重置。
    degraded_logged: bool,

    // ===== WebView 链路状态（原 WM） =====
    /// monitor_id -> window_label 映射。
    windows: HashMap<String, String>,
}

/// extend 组降级时使用的 code；前端据此改用 warning 级 toast。
const CODE_EXTEND_DEGRADED: &str = "ExtendDegraded";

impl UnifiedWallpaperManager {
    pub async fn new(db: DatabaseConnection, app_handle: AppHandle) -> Result<Self> {
        // 失败上报与设备丢失收尾统一在 errors 模块；渲染层只持有回调，
        // 不感知 AppHandle / AppContext（见 errors.rs 模块说明）。
        let renderer = DesktopRenderer::start(errors::device_lost_handler(&app_handle))?;
        let mut manager = Self {
            db,
            app_handle,
            slots: HashMap::new(),
            observed: ConfigSnapshot::default(),
            renderer,
            selection: default_backend_selection(),
            backends: HashMap::new(),
            wallpaper_ids: HashMap::new(),
            rects: HashMap::new(),
            display_mode: DisplayMode::Independent,
            degraded_logged: false,
            windows: HashMap::new(),
        };
        // 启动时同步一次全局 display_mode / volume，避免 extend 组首屏仍按
        // independent 渲染，以及持久化音量在重启后失效。此时对象尚未进入 Mutex，
        // 无并发访问，直接初始化即可。
        manager.refresh_display_mode().await;
        manager.refresh_volume().await;

        Ok(manager)
    }

    /// 一次性初始化：建立初始快照 + 按系统物理屏同步 config / slot / 渲染目标。
    ///
    /// 非 Windows 平台 `enumerate_monitors()` 返回空集，只构建快照，
    /// 现有前端链路照常工作，macOS 开发构建行为不变。
    pub async fn init(&mut self) -> Result<()> {
        let configs = monitor_config_service::get_all(&self.db).await?;
        let display_mode = app_setting_service::get(&self.db, setting_keys::DISPLAY_MODE)
            .await?
            .unwrap_or_else(|| DEFAULT_DISPLAY_MODE.to_string());
        let volume = app_setting_service::get(&self.db, setting_keys::GLOBAL_VOLUME)
            .await?
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0);
        self.observed = build_snapshot(&configs, display_mode, volume);

        // M20 规则 1：init 枚举到的物理屏必定 active=true（新屏 insert，消失的置 false）。
        // sync_monitors_from_system 内部按「DB → slot → reconcile」顺序执行。
        self.sync_monitors_from_system().await?;
        Ok(())
    }

    /// 重算整组 viewport 并按 monitor_id 定向推送给每个壁纸窗口。
    ///
    /// 用 Rust 侧 `GetDpiForMonitor` 拿到的真实 DPI 把物理坐标换算成 CSS 像素，
    /// 前端直接用于 style、零计算——这是 M18 的最终修复。
    /// 只在 slot 几何变化（init / 热插拔）与 display_mode 切换时调用，不进每帧热路径。
    pub async fn push_extend_viewports(&self) {
        let monitors: Vec<(String, MonitorRect)> = self
            .slots
            .iter()
            .map(|(id, slot)| (id.clone(), slot.rect))
            .collect();
        let Some(viewports) = group_viewports(&monitors) else {
            return;
        };
        for (monitor_id, viewport) in viewports {
            let Some(slot) = self.slots.get(&monitor_id) else {
                continue;
            };
            if !slot.has_target {
                continue;
            }
            let label = webview::wallpaper_window_label(&monitor_id);
            let payload = ExtendViewportPayload::from_physical(viewport, slot.dpi);
            if let Err(err) = self.app_handle.typed_emit_to(&label, &payload) {
                log::warn!(
                    "[UnifiedWallpaperManager] 推送 viewport 失败 {}: {}",
                    monitor_id,
                    err
                );
            }
        }
    }

    /// 供壁纸页挂载时主动拉取一次当前 viewport（事件可能在 React 订阅前发出）。
    pub fn current_extend_viewport(&self, monitor_id: &str) -> Option<ExtendViewportPayload> {
        let slot = self.slots.get(monitor_id)?;
        let monitors: Vec<(String, MonitorRect)> = self
            .slots
            .iter()
            .map(|(id, slot)| (id.clone(), slot.rect))
            .collect();
        let viewports = group_viewports(&monitors)?;
        let (_, viewport) = viewports.into_iter().find(|(id, _)| id == monitor_id)?;
        Some(ExtendViewportPayload::from_physical(viewport, slot.dpi))
    }

    pub fn observed(&self) -> &ConfigSnapshot {
        &self.observed
    }

    pub fn slots(&self) -> &HashMap<String, MonitorSlot> {
        &self.slots
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::monitor_config::Model;

    fn config(monitor_id: &str, wid: Option<i32>, active: bool) -> Model {
        Model {
            id: 1,
            monitor_id: monitor_id.to_string(),
            wallpaper_id: wid,
            collection_id: None,
            fit_mode: "cover".to_string(),
            play_mode: "sequential".to_string(),
            play_interval: 300,
            is_enabled: true,
            active,
            updated_at: String::new(),
        }
    }

    #[test]
    fn snapshot_only_keeps_window_fields_and_global_whitelist() {
        let configs = vec![
            config("DISPLAY1", Some(7), true),
            config("DISPLAY2", None, false),
        ];
        let snapshot = build_snapshot(&configs, "extend".to_string(), 55);

        assert_eq!(snapshot.monitors.len(), 2);
        assert_eq!(snapshot.monitors["DISPLAY1"].wallpaper_id, Some(7));
        assert!(snapshot.monitors["DISPLAY1"].active);
        assert!(!snapshot.monitors["DISPLAY2"].active);
        assert_eq!(snapshot.global.display_mode, "extend");
        assert_eq!(snapshot.global.volume, 55);
    }

    #[test]
    fn snapshot_of_empty_configs_keeps_default_global() {
        let snapshot = build_snapshot(&[], DEFAULT_DISPLAY_MODE.to_string(), 0);
        assert!(snapshot.monitors.is_empty());
        assert_eq!(snapshot.global.display_mode, DEFAULT_DISPLAY_MODE);
    }
}
