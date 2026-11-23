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
use std::sync::Arc;

use anyhow::Result;
use sea_orm::DatabaseConnection;
use tokio::sync::Mutex;

use crate::ctx::reconcile::{
    ConfigSnapshot, GlobalSnapshot, MonitorSnapshot, DEFAULT_DISPLAY_MODE,
};
use crate::ctx::render_coordinator::RenderCoordinator;
use crate::ctx::window_manager::WallpaperWindowManager;
use crate::dto::app_setting_dto::keys as setting_keys;
use crate::platform::wallpaper_renderer::MonitorRect as RenderRect;
use crate::services::{app_setting_service, monitor_config_service};

#[cfg(target_os = "windows")]
use crate::platform::windows::monitor_geometry::enumerate_monitors;

/// 单个物理显示器在 Manager 内的登记项。
///
/// `rect` 恒为物理像素虚拟桌面坐标；`dpi` 来自目标屏自身，用于 WebView
/// 建窗时把物理像素换算成 Tauri builder 的逻辑像素（M18）。
#[derive(Debug, Clone)]
pub struct MonitorSlot {
    pub rect: RenderRect,
    pub dpi: u32,
    pub has_target: bool,
}

/// 把 Win32 的 `MONITORINFOEXW` 矩形转成渲染契约使用的 x/y/width/height。
/// 两套 MonitorRect 同名但语义不同，转换集中在此处，避免散落各处手写。
#[cfg(target_os = "windows")]
fn to_render_rect(
    info: &crate::platform::windows::monitor_geometry::MonitorInfo,
) -> (String, RenderRect, u32) {
    (
        info.rect.device_name.clone(),
        RenderRect {
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

pub struct UnifiedWallpaperManager {
    db: DatabaseConnection,
    coordinator: Arc<Mutex<RenderCoordinator>>,
    /// 供 Phase 3 统一派发 WebView 侧动作；当前 init 仍经 coordinator 间接使用。
    window_manager: Arc<Mutex<WallpaperWindowManager>>,
    slots: HashMap<String, MonitorSlot>,
    /// 已成功应用的窗口状态，init 完成后作为后续 reconcile 的起点。
    observed: ConfigSnapshot,
}

impl UnifiedWallpaperManager {
    pub fn new(
        db: DatabaseConnection,
        coordinator: Arc<Mutex<RenderCoordinator>>,
        window_manager: Arc<Mutex<WallpaperWindowManager>>,
    ) -> Self {
        Self {
            db,
            coordinator,
            window_manager,
            slots: HashMap::new(),
            observed: ConfigSnapshot::default(),
        }
    }

    /// 一次性初始化：Win32 枚举物理屏 ∩ DB config → 登记 slot → 建渲染目标 → 生成 observed。
    ///
    /// 刻意**只读 DB、不写 DB**：新屏登记（插入 config / 失活消失的屏）属于写入源收口，
    /// 与 Phase 3 停用前端 `doSyncMonitors` 一起做。首次启动 DB 为空时由前端原有路径
    /// 完成登记与建窗，Manager 下一次启动再接管，避免出现「两边都写」的中间态。
    ///
    /// 非 Windows 平台 `enumerate_monitors()` 返回空集，init 退化为只构建快照，
    /// 现有前端链路照常工作，因此 macOS 开发构建行为不变。
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

        #[cfg(target_os = "windows")]
        {
            let by_id: HashMap<_, _> = configs
                .iter()
                .map(|config| (config.monitor_id.as_str(), config))
                .collect();

            for info in enumerate_monitors() {
                let (monitor_id, rect, dpi) = to_render_rect(&info);
                let Some(config) = by_id.get(monitor_id.as_str()) else {
                    // DB 尚无该屏配置，交给前端原有登记路径，不在 init 里写 DB。
                    continue;
                };
                if !config.active {
                    continue;
                }

                // register_monitor 必须在判断「有没有壁纸」之前调用：
                // 无壁纸的屏也要进 extend union，否则虚拟桌面画布缺一块。
                let has_target = {
                    let mut coordinator = self.coordinator.lock().await;
                    coordinator.register_monitor(&monitor_id, rect);
                    match config.wallpaper_id {
                        Some(wallpaper_id) => coordinator
                            .ensure_target(&monitor_id, rect, wallpaper_id)
                            .await
                            .is_ok(),
                        None => false,
                    }
                };

                self.slots.insert(
                    monitor_id,
                    MonitorSlot {
                        rect,
                        dpi,
                        has_target,
                    },
                );
            }
        }

        #[cfg(not(target_os = "windows"))]
        let _ = &configs;

        Ok(())
    }

    pub fn observed(&self) -> &ConfigSnapshot {
        &self.observed
    }

    pub fn slots(&self) -> &HashMap<String, MonitorSlot> {
        &self.slots
    }

    pub fn window_manager(&self) -> &Arc<Mutex<WallpaperWindowManager>> {
        &self.window_manager
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
