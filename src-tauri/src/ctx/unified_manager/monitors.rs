//! 显示器同步：系统枚举 → DB 写入 → slot 更新 → reconcile。
//!
//! 从 mod.rs 拆出的独立功能块；顺序约束（DB → slot → reconcile）是硬性的，
//! 见 sync_monitors_from_system 的文档注释。

use anyhow::Result;

#[cfg(target_os = "windows")]
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use crate::ctx::unified_manager::{to_render_rect, MonitorSlot};
#[cfg(target_os = "windows")]
use crate::dto::monitor_config_dto::UpsertMonitorConfigRequest;
#[cfg(target_os = "windows")]
use crate::platform::windows::monitor_geometry::enumerate_monitors;
#[cfg(target_os = "windows")]
use crate::services::monitor_config_service;

impl super::UnifiedWallpaperManager {
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
            self.reconcile(crate::ctx::unified_manager::ReconcileScope::All)
                .await?;
            // viewport 需要在 target 变化后重推；reconcile 内部已按动作推过，
            // 这里补一次保证 slot 几何刚变化的新屏也拿到最新 union。
            self.push_extend_viewports().await;
            Ok(())
        }
    }
}
