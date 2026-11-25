//! reconcile 编排：读 DB 全量快照 → diff → 逐项派发 → 局部推进 observed。
//!
//! 从 mod.rs 拆出的独立功能块；纯算法在 `engine.rs`，这里只做编排与动作执行。
//! 失败项不推进 observed，下一轮 reconcile 自动重放（C2）。

use anyhow::Result;

use crate::ctx::unified_manager::engine::{
    diff_snapshots, patch_observed, plan_dispatch, DispatchAction, ReconcileScope,
    DEFAULT_DISPLAY_MODE,
};
use crate::dto::app_setting_dto::keys as setting_keys;
use crate::events::VolumeChangedPayload;
use crate::platform::wallpaper_renderer::DisplayMode;
use crate::services::{app_setting_service, monitor_config_service};

use super::{action_monitor_id, build_snapshot};

impl super::UnifiedWallpaperManager {
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
                crate::ctx::unified_manager::engine::ConfigDiff::DisplayModeChanged { .. }
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
}
