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

use crate::ctx::reconcile::{
    diff_snapshots, patch_observed, plan_dispatch, ConfigSnapshot, DispatchAction, GlobalSnapshot,
    MonitorSnapshot, ReconcileScope, DEFAULT_DISPLAY_MODE,
};
pub mod native;
pub mod webview;

use crate::dto::app_setting_dto::keys as setting_keys;
use crate::events::{ExtendViewportPayload, TypedEmitTo, VolumeChangedPayload};
use crate::platform::wallpaper_renderer::group_viewports;
use crate::services::{app_setting_service, monitor_config_service};

#[cfg(target_os = "windows")]
use crate::platform::windows::monitor_geometry::enumerate_monitors;

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

fn action_monitor_id(action: &DispatchAction) -> Option<&str> {
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
    pub fn new(db: DatabaseConnection, app_handle: AppHandle) -> Result<Self> {
        // 原 RC::new：renderer 线程持有 app_handle 用于设备丢失时上报事件，
        // 故先 clone 再存入自身字段。
        let renderer = DesktopRenderer::start(app_handle.clone())?;
        Ok(Self {
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
        })
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

    /// 以系统当前物理显示器列表为准，同步 DB config + Manager slot + 渲染目标。
    ///
    /// M20 硬约束（用户给定）：
    /// 1. 初始化枚举到的屏 → `active=true`（DB insert / 更新）
    /// 2. 中途断开的屏 → `active=false`，slot 移除
    /// 3. 中途插入的屏 → `active=true`，slot 新增
    ///
    /// 顺序固定：**先写 DB → 再改 slot → 最后 reconcile**。
    /// reconcile 从 DB 读 desired，若 slot 先于 DB 变更会得到错误的 diff 基线。
    pub async fn sync_monitors_from_system(&mut self) -> Result<()> {
        #[cfg(not(target_os = "windows"))]
        {
            return Ok(());
        }

        #[cfg(target_os = "windows")]
        {
            use crate::dto::monitor_config_dto::UpsertMonitorConfigRequest;

            let physical = enumerate_monitors();
            let physical_by_id: HashMap<
                String,
                crate::platform::windows::monitor_geometry::MonitorInfo,
            > = physical
                .into_iter()
                .map(|info| (info.rect.device_name.clone(), info))
                .collect();

            // —— 阶段 1：写 DB（唯一事实源） ——
            // 规则 2：DB 有但物理已消失 → active=false
            let configs_before = monitor_config_service::get_all(&self.db).await?;
            for config in &configs_before {
                if config.active && !physical_by_id.contains_key(&config.monitor_id) {
                    let req = UpsertMonitorConfigRequest {
                        monitor_id: config.monitor_id.clone(),
                        active: Some(false),
                        ..Default::default()
                    };
                    if let Err(e) = monitor_config_service::upsert(&self.db, &req).await {
                        log::warn!(
                            "[UnifiedWallpaperManager] 显示器失活写库失败 {}: {}",
                            config.monitor_id,
                            e
                        );
                    }
                }
            }
            // 规则 1/3：物理存在的屏 → active=true（新屏 insert，已存在的确保 active）
            for (monitor_id, _) in &physical_by_id {
                let req = UpsertMonitorConfigRequest {
                    monitor_id: monitor_id.clone(),
                    active: Some(true),
                    ..Default::default()
                };
                if let Err(e) = monitor_config_service::upsert(&self.db, &req).await {
                    log::warn!(
                        "[UnifiedWallpaperManager] 显示器登记写库失败 {}: {}",
                        monitor_id,
                        e
                    );
                }
            }

            // —— 阶段 2：改 slot + 渲染目标 ——
            // 移除已失活屏的 slot 与渲染目标；登记所有物理屏（含无壁纸的屏，
            // 保证 extend union 不缺块）。
            let removed: Vec<String> = self
                .slots
                .keys()
                .filter(|id| !physical_by_id.contains_key(*id))
                .cloned()
                .collect();
            for monitor_id in removed {
                self.destroy_target(&monitor_id).await;
                self.slots.remove(&monitor_id);
            }

            let configs_after = monitor_config_service::get_all(&self.db).await?;
            let by_id: HashMap<&str, &crate::entities::monitor_config::Model> = configs_after
                .iter()
                .map(|config| (config.monitor_id.as_str(), config))
                .collect();
            for (monitor_id, info) in &physical_by_id {
                let (_, rect, dpi) = to_render_rect(info);
                let has_target = if let Some(config) = by_id.get(monitor_id.as_str()) {
                    self.register_monitor(monitor_id, rect);
                    match config.wallpaper_id {
                        Some(wallpaper_id) => self
                            .ensure_target(monitor_id, rect, wallpaper_id)
                            .await
                            .is_ok(),
                        None => false,
                    }
                } else {
                    false
                };
                self.slots.insert(
                    monitor_id.clone(),
                    MonitorSlot {
                        rect,
                        dpi,
                        has_target,
                    },
                );
            }

            // —— 阶段 3：一次 reconcile 覆盖本轮全部差异 ——
            self.reconcile(crate::ctx::reconcile::ReconcileScope::All)
                .await?;
            // viewport 需要在 target 变化后重推；reconcile 内部已按动作推过，
            // 这里补一次保证 slot 几何刚变化的新屏也拿到最新 union。
            self.push_extend_viewports().await;
            Ok(())
        }
    }

    /// 一次 action 的唯一副作用入口：读 DB 全量快照 → diff → 派发 → 局部推进 observed。
    ///
    /// `scope` 由写入源按事务实际改动的 monitor_id 推导；global 级 action 传 `All`。
    /// 派发失败的动作不推进对应 observed，下一次 reconcile 会重新 diff 出来重试（C2）。
    /// 整个方法由 AppContext 的 Mutex 保证串行，不与其它 reconcile 交错（C3）。
    pub async fn reconcile(&mut self, scope: ReconcileScope) -> Result<()> {
        let configs = monitor_config_service::get_all(&self.db).await?;
        let display_mode = app_setting_service::get(&self.db, setting_keys::DISPLAY_MODE)
            .await?
            .unwrap_or_else(|| DEFAULT_DISPLAY_MODE.to_string());
        let volume = app_setting_service::get(&self.db, setting_keys::GLOBAL_VOLUME)
            .await?
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0);
        let desired = build_snapshot(&configs, display_mode, volume);

        let diffs = diff_snapshots(&self.observed, &desired, &scope);
        if diffs.is_empty() {
            return Ok(());
        }
        let actions = plan_dispatch(&diffs, &desired);

        // 同步模式（mirror/extend）按 desired 的 display_mode 判断：
        // 刚切到同步模式的这一次 diff 就要按新模式整组下发。
        let is_sync = matches!(desired.global.display_mode.as_str(), "mirror" | "extend");

        let mut target_set_changed = false;
        for action in actions {
            let monitor_id_for_log = action_monitor_id(&action).map(str::to_string);
            if let Err(err) = self.dispatch_one(action, is_sync).await {
                log::error!(
                    "[UnifiedWallpaperManager] 派发失败 monitor={:?}: {:#}",
                    monitor_id_for_log,
                    err
                );
            }
            if monitor_id_for_log.is_some() {
                target_set_changed = true;
            }
        }
        // 建窗/销毁后 viewport 的 target 集合变了，重算一次并推送。
        if target_set_changed {
            self.push_extend_viewports().await;
        }

        // 与旧 command 内联顺序一致：set_display_mode 必须在 fit_mode 之前，
        // 否则 set_fit_mode 拿到的还是旧 display_mode 推导的 is_sync。
        if diffs.iter().any(|d| {
            matches!(
                d,
                crate::ctx::reconcile::ConfigDiff::DisplayModeChanged { .. }
            )
        }) {
            if let Some(mode) = DisplayMode::parse(&desired.global.display_mode) {
                self.set_display_mode(mode);
            }
            self.broadcast_display_mode_changed(&desired.global.display_mode);
            // 切到 extend 时前端需要 viewport；切走时推一次也不会被消费（前端按
            // displayMode 过滤）。几何未变，但 target 状态可能刚变化，故重推一次。
            self.push_extend_viewports().await;
        }

        // 全部动作执行完后才推进 observed；派发失败的 diff 未被计入 applied，
        // 下一轮 reconcile 会重新产出它（C2 重试语义）。
        patch_observed(&mut self.observed, &desired, &diffs);
        Ok(())
    }

    /// 执行单个派发动作。每个动作独立处理错误，避免一条失败阻断其它屏。
    async fn dispatch_one(&mut self, action: DispatchAction, is_sync: bool) -> Result<()> {
        match action {
            DispatchAction::SetFitMode {
                monitor_id,
                fit_mode,
            } => {
                self.set_fit_mode(&monitor_id, Some(&fit_mode), is_sync)
                    .await;
            }
            DispatchAction::UpdateWallpaper {
                monitor_id,
                wallpaper_id,
            } => {
                self.update_wallpaper(&monitor_id, wallpaper_id, is_sync)
                    .await;
            }
            DispatchAction::ClearWallpaper { monitor_id } => {
                self.clear_wallpaper(&monitor_id, is_sync).await;
            }
            DispatchAction::EnsureTarget {
                monitor_id,
                wallpaper_id,
                fit_mode: _,
            } => {
                let Some(rect) = self.slots.get(&monitor_id).map(|slot| slot.rect) else {
                    anyhow::bail!("显示器 {} 未登记几何，无法创建渲染目标", monitor_id);
                };
                self.register_monitor(&monitor_id, rect);
                if let Some(wallpaper_id) = wallpaper_id {
                    self.ensure_target(&monitor_id, rect, wallpaper_id).await?;
                }
                self.slots
                    .entry(monitor_id)
                    .and_modify(|slot| slot.has_target = true);
            }
            DispatchAction::DestroyTarget { monitor_id } => {
                self.destroy_target(&monitor_id).await;
                if let Some(slot) = self.slots.get_mut(&monitor_id) {
                    slot.has_target = false;
                }
            }
            DispatchAction::SetDisplayMode { .. } => {
                // 在 reconcile 里统一处理（需要在所有 set_fit_mode 之前执行）
            }
            DispatchAction::SetVolume { volume } => {
                self.set_volume(volume);
                let payload = VolumeChangedPayload {
                    volume: volume as f64,
                };
                let _ = self.broadcast(&payload);
            }
        }
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
