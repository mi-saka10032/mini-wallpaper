//! 轮播定时器业务模块
//!
//! 本模块负责轮播切换的业务逻辑：
//! - `CarouselTask`：轮播任务定义，实现 `TaskSpawner` trait
//! - `should_start_timer()`: 纯逻辑判断，是否满足轮播启动条件
//! - `collection_has_enough_wallpapers()`: 检查收藏夹壁纸数量
//!
//! 定时器生命周期由 `Scheduler` 统一管理，
//! `CarouselTask` 自身持有 `AppHandle` + 纯业务参数 `monitor_id`，
//! 在 `spawn` 内部通过句柄按需获取 db / window_manager 等共享资源。

use log::{error, info, warn};
use sea_orm::DatabaseConnection;
use tauri::{Emitter, Manager};
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};

use super::scheduler::TaskSpawner;
use crate::ctx::AppContext;
use crate::entities::monitor_config;
use crate::services::{app_setting_service, collection_service, monitor_config_service};

/// 轮播定时器在 Scheduler 中的 key 前缀
pub const CAROUSEL_TIMER_PREFIX: &str = "carousel:";

/// 生成轮播定时器的 scheduler key
pub fn carousel_key(monitor_id: &str) -> String {
    format!("{}{}", CAROUSEL_TIMER_PREFIX, monitor_id)
}

/// 缩略图变更事件 payload（发送给主窗口更新缩略图）
#[derive(Clone, serde::Serialize)]
pub struct ThumbnailChangedPayload {
    pub monitor_id: String,
    pub wallpaper_id: i32,
}

/// 轮播任务定义
///
/// 自身持有 `AppHandle`（用于按需获取 db / window_manager / emit 事件）
/// 和纯业务参数 `monitor_id`，`spawn` 消费 self 即可启动，无需外部注入。
pub struct CarouselTask {
    pub app: tauri::AppHandle,
    pub monitor_id: String,
}

impl TaskSpawner for CarouselTask {
    fn spawn(self) -> JoinHandle<()> {
        let app = self.app;
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
                let display_mode = match app_setting_service::get(&db, "display_mode").await {
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
                        // 判断是否为 mirror/extend 模式，需要同步更新所有窗口
                        let is_sync_mode = display_mode == "mirror" || display_mode == "extend";

                        if is_sync_mode {
                            // mirror/extend 模式：遍历所有 active monitor，同步更新 wallpaper_id 和壁纸窗口
                            let all_configs = match monitor_config_service::get_all(&db).await {
                                Ok(c) => c,
                                Err(e) => {
                                    error!("[Carousel] Failed to get all configs: {}", e);
                                    continue;
                                }
                            };

                            for config in &all_configs {
                                if !config.active {
                                    continue;
                                }
                                // 更新每个 active monitor 的 wallpaper_id
                                if let Err(e) = monitor_config_service::update_wallpaper_id(
                                    &db,
                                    &config.monitor_id,
                                    new_wid,
                                ).await {
                                    error!(
                                        "[Carousel] Failed to update wallpaper_id for {}: {}",
                                        config.monitor_id, e
                                    );
                                }

                                // 通知每个壁纸窗口更新
                                let wm_guard = window_manager.lock().await;
                                if let Err(e) = wm_guard.update_window(&config.monitor_id, new_wid) {
                                    warn!("[Carousel] 壁纸窗口更新失败 {}: {}", config.monitor_id, e);
                                }
                                drop(wm_guard);

                                // 通知主窗口更新缩略图
                                let payload = ThumbnailChangedPayload {
                                    monitor_id: config.monitor_id.clone(),
                                    wallpaper_id: new_wid,
                                };
                                if let Err(e) = app.emit("thumbnail-changed", &payload) {
                                    error!("[Carousel] Failed to emit thumbnail-changed for {}: {}", config.monitor_id, e);
                                }
                            }

                            info!("[Carousel] {} 模式同步更新所有窗口: wallpaper_id={}", display_mode, new_wid);
                        } else {
                            // independent 模式：仅更新当前 monitor
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
                            let wm_guard = window_manager.lock().await;
                            if let Err(e) = wm_guard.update_window(&mid, new_wid) {
                                warn!("[Carousel] 壁纸窗口更新失败: {}", e);
                            }
                            drop(wm_guard);

                            // 2. 通知主窗口更新缩略图（全局广播）
                            let payload = ThumbnailChangedPayload {
                                monitor_id: mid.clone(),
                                wallpaper_id: new_wid,
                            };
                            if let Err(e) = app.emit("thumbnail-changed", &payload) {
                                error!("[Carousel] Failed to emit thumbnail-changed: {}", e);
                            }
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

/// 判断是否满足轮播启动条件（纯逻辑判断，不涉及 DB）
///
/// 仅检查 active + is_enabled 两个开关条件，
/// collection_id 是否存在由调用方在具体场景中额外判断。
pub fn should_start_timer(config: &monitor_config::Model) -> bool {
    config.active && config.is_enabled
}

/// 检查收藏夹壁纸数量是否 > 1（委托 collection_service）
pub async fn collection_has_enough_wallpapers(
    db: &DatabaseConnection,
    collection_id: i32,
) -> anyhow::Result<bool> {
    let count = collection_service::count_wallpapers(db, collection_id).await?;
    Ok(count > 1)
}
