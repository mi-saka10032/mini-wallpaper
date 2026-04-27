//! 轮播定时器业务模块
//!
//! 本模块仅负责轮播切换的业务逻辑：
//! - `spawn_carousel_task()`: 工厂函数，创建轮播异步任务并返回 JoinHandle
//! - `should_start_timer()`: 纯逻辑判断，是否满足轮播启动条件
//! - `collection_has_enough_wallpapers()`: 检查收藏夹壁纸数量
//!
//! 定时器生命周期由 `TimerRegistry` 统一管理，本模块不持有任何句柄。

use log::{error, warn};
use sea_orm::DatabaseConnection;
use tauri::{Emitter, Manager};
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};

use crate::entities::monitor_config;
use crate::services::{collection_service, monitor_config_service};
use crate::services::wallpaper_window_service::WallpaperWindowManagerState;

/// 轮播定时器在 TimerRegistry 中的 key 前缀
pub const CAROUSEL_TIMER_PREFIX: &str = "carousel:";

/// 生成轮播定时器的 registry key
pub fn carousel_key(monitor_id: &str) -> String {
    format!("{}{}", CAROUSEL_TIMER_PREFIX, monitor_id)
}

/// 缩略图变更事件 payload（发送给主窗口更新缩略图）
#[derive(Clone, serde::Serialize)]
pub struct ThumbnailChangedPayload {
    pub monitor_id: String,
    pub wallpaper_id: i32,
}

/// 创建轮播切换异步任务（工厂函数）
///
/// 返回 `JoinHandle`，由调用方注册到 `TimerRegistry`。
/// 任务内部按 config 中的 play_interval 定时切换壁纸。
pub fn spawn_carousel_task(
    monitor_id: String,
    db: DatabaseConnection,
    app_handle: tauri::AppHandle,
) -> JoinHandle<()> {
    let mid = monitor_id.clone();

    tokio::spawn(async move {
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

        let play_mode = config.play_mode.clone();
        let seconds = config.play_interval.max(5) as u64; // 最小 5 秒

        let mut tick = interval(Duration::from_secs(seconds));
        // 第一次 tick 立即触发，跳过
        tick.tick().await;

        loop {
            tick.tick().await;

            // 获取当前 config 最新的 wallpaper_id
            let current_wid = match monitor_config_service::get_by_monitor_id(&db, &mid).await {
                Ok(Some(c)) => c.wallpaper_id,
                Ok(None) => {
                    warn!("[Carousel] Config not found for {}, stopping", mid);
                    break;
                }
                Err(e) => {
                    error!("[Carousel] Failed to get config for {}: {}", mid, e);
                    continue;
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
                    // 通过 monitor_config_service 更新 wallpaper_id
                    if let Err(e) =
                        monitor_config_service::update_wallpaper_id(&db, &mid, new_wid).await
                    {
                        error!(
                            "[Carousel] Failed to update wallpaper_id for {}: {}",
                            mid, e
                        );
                        continue;
                    }

                    // 1. 通知指定壁纸窗口更新壁纸（精确定向发送）
                    let ww_manager = app_handle.state::<WallpaperWindowManagerState>();
                    let wm_guard = ww_manager.lock().await;
                    if let Err(e) = wm_guard.update_window(&app_handle, &mid, new_wid) {
                        warn!("[Carousel] 壁纸窗口更新失败: {}", e);
                    }
                    drop(wm_guard);

                    // 2. 通知主窗口更新缩略图（全局广播）
                    let payload = ThumbnailChangedPayload {
                        monitor_id: mid.clone(),
                        wallpaper_id: new_wid,
                    };
                    if let Err(e) = app_handle.emit("thumbnail-changed", &payload) {
                        error!("[Carousel] Failed to emit thumbnail-changed: {}", e);
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

/// 判断是否满足轮播启动条件（纯逻辑判断，不涉及 DB）
pub fn should_start_timer(config: &monitor_config::Model) -> bool {
    config.active && config.is_enabled && config.collection_id.is_some()
}

/// 检查收藏夹壁纸数量是否 > 1（委托 collection_service）
pub async fn collection_has_enough_wallpapers(
    db: &DatabaseConnection,
    collection_id: i32,
) -> anyhow::Result<bool> {
    let count = collection_service::count_wallpapers(db, collection_id).await?;
    Ok(count > 1)
}
