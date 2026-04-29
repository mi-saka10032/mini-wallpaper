//! 轮播定时器业务模块
//!
//! 本模块负责轮播切换的业务逻辑：
//! - `CarouselTask`：轮播任务定义，实现 `TaskSpawner` trait
//! - `carousel_key()`：生成轮播定时器的 scheduler key
//!
//! 定时器生命周期由 `Scheduler` 统一管理，
//! `CarouselTask` 仅持有纯业务参数 `monitor_id`，
//! `spawn` 接收调度器注入的 `AppHandle`，按需获取 db / window_manager 等共享资源。

use log::{error, info, warn};
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};

use sea_orm::DatabaseConnection;

use super::scheduler::TaskSpawner;
use crate::ctx::AppContext;
use crate::ctx::window_manager::WallpaperWindowManager;
use crate::dto::app_setting_dto::keys as setting_keys;
use crate::events::{ThumbnailChangedPayload, TypedEmit};
use crate::services::{app_setting_service, collection_service, monitor_config_service};

/// 轮播定时器在 Scheduler 中的 key 前缀
pub const CAROUSEL_TIMER_PREFIX: &str = "carousel:";

/// 生成轮播定时器的 scheduler key
pub fn carousel_key(monitor_id: &str) -> String {
    format!("{}{}", CAROUSEL_TIMER_PREFIX, monitor_id)
}

/// 轮播任务定义
///
/// 仅持有纯业务参数 `monitor_id`，`spawn` 接收调度器注入的 `AppHandle`，
/// 按需获取 db / window_manager / emit 事件等共享资源。
pub struct CarouselTask {
    pub monitor_id: String,
}

/// 对单个 monitor 执行壁纸切换：更新 DB + 通知壁纸窗口 + 通知主窗口缩略图
async fn apply_wallpaper_change(
    db: &DatabaseConnection,
    window_manager: &Arc<Mutex<WallpaperWindowManager>>,
    app: &tauri::AppHandle,
    monitor_id: &str,
    new_wid: i32,
) {
    // 1. 更新 DB 中的 wallpaper_id
    if let Err(e) = monitor_config_service::update_wallpaper_id(db, monitor_id, new_wid).await {
        error!(
            "[Carousel] Failed to update wallpaper_id for {}: {}",
            monitor_id, e
        );
        return;
    }

    // 2. 通知壁纸窗口更新
    {
        let wm_guard = window_manager.lock().await;
        if let Err(e) = wm_guard.update_window(monitor_id, new_wid) {
            warn!("[Carousel] 壁纸窗口更新失败 {}: {}", monitor_id, e);
        }
    }

    // 3. 通知主窗口更新缩略图
    let payload = ThumbnailChangedPayload {
        monitor_id: monitor_id.to_string(),
        wallpaper_id: new_wid,
    };
    if let Err(e) = app.typed_emit(&payload) {
        error!(
            "[Carousel] Failed to emit thumbnail-changed for {}: {}",
            monitor_id, e
        );
    }
}

impl TaskSpawner for CarouselTask {
    fn spawn(self, app: &tauri::AppHandle) -> JoinHandle<()> {
        let app = app.clone();
        let mid = self.monitor_id;

        tokio::spawn(async move {
            // 从 AppHandle 按需获取共享资源
            let ctx = app.state::<AppContext>();
            let db = ctx.db.clone();
            let window_manager = ctx.window_manager.clone();

            // 查询 config 获取轮播参数
            let config = match monitor_config_service::get_by_monitor_id(&db, &mid).await {
                Ok(Some(c)) => c,
                Ok(None) => {
                    warn!("[Carousel] Config not found for {}, aborting start", mid);
                    return;
                }
                Err(e) => {
                    error!("[Carousel] Failed to get config for {}: {}", mid, e);
                    return;
                }
            };

            let collection_id = match config.collection_id {
                Some(cid) => cid,
                None => {
                    warn!("[Carousel] No collection_id for {}, aborting start", mid);
                    return;
                }
            };

            let seconds = config.play_interval.max(5) as u64; // 最小 5 秒

            let mut tick = interval(Duration::from_secs(seconds));
            // 第一次 tick 立即触发，跳过
            tick.tick().await;

            loop {
                tick.tick().await;

                // 每次 tick 动态拉取最新 config，获取 play_mode 和 wallpaper_id
                let current_config =
                    match monitor_config_service::get_by_monitor_id(&db, &mid).await {
                        Ok(Some(c)) => c,
                        Ok(None) => {
                            warn!("[Carousel] Config not found for {}, stopping", mid);
                            break;
                        }
                        Err(e) => {
                            error!("[Carousel] Failed to get config for {}: {}", mid, e);
                            continue;
                        }
                    };

                let current_wid = current_config.wallpaper_id;
                let play_mode = current_config.play_mode;

                // 读取全局 display_mode 设置
                let display_mode =
                    match app_setting_service::get(&db, setting_keys::DISPLAY_MODE).await {
                        Ok(Some(dm)) => dm,
                        Ok(None) => "independent".to_string(),
                        Err(e) => {
                            warn!("[Carousel] Failed to get display_mode: {}", e);
                            "independent".to_string()
                        }
                    };

                // 通过 collection_service 获取下一张壁纸
                match collection_service::next_wallpaper_id(
                    &db,
                    collection_id,
                    current_wid,
                    &play_mode,
                )
                .await
                {
                    Ok(Some(new_wid)) => {
                        let is_sync_mode = display_mode == "mirror" || display_mode == "extend";

                        if is_sync_mode {
                            // mirror/extend 模式：遍历所有 active monitor，同步更新
                            let all_configs = match monitor_config_service::get_all(&db).await {
                                Ok(c) => c,
                                Err(e) => {
                                    error!("[Carousel] Failed to get all configs: {}", e);
                                    continue;
                                }
                            };

                            for config in &all_configs {
                                if config.active {
                                    apply_wallpaper_change(
                                        &db,
                                        &window_manager,
                                        &app,
                                        &config.monitor_id,
                                        new_wid,
                                    )
                                    .await;
                                }
                            }

                            info!(
                                "[Carousel] {} 模式同步更新所有窗口: wallpaper_id={}",
                                display_mode, new_wid
                            );
                        } else {
                            // independent 模式：仅更新当前 monitor
                            apply_wallpaper_change(
                                &db,
                                &window_manager,
                                &app,
                                &mid,
                                new_wid,
                            )
                            .await;
                        }
                    }
                    Ok(None) => {
                        // 收藏夹为空或只有一张，无需切换
                    }
                    Err(e) => {
                        error!(
                            "[Carousel] Error getting next wallpaper for {}: {}",
                            mid, e
                        );
                    }
                }
            }
        })
    }
}
