//! 轮播定时器业务模块
//!
//! 本模块负责轮播切换的业务逻辑：
//! - `CarouselTask`：轮播任务定义，实现 `TaskSpawner` trait
//! - `carousel_key()`：生成轮播定时器的 scheduler key
//!
//! 定时器生命周期由 `Scheduler` 统一管理，
//! `CarouselTask` 仅持有纯业务参数 `monitor_id`，
//! `spawn` 接收调度器注入的 `AppHandle`，按需获取 db / window_manager 等共享资源。

use log::{error, warn};
use sea_orm::TransactionTrait;
use tauri::Manager;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};

use crate::ctx::reconcile::ReconcileScope;
use crate::ctx::AppContext;
use crate::dto::app_setting_dto::keys as setting_keys;
use crate::runtime::scheduler::TaskSpawner;
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
    /// 驱动哪块显示器的轮播；同时是 scheduler key 的一部分（carousel:<monitor_id>）。
    pub monitor_id: String,
}

impl TaskSpawner for CarouselTask {
    /// 每块显示器一个独立任务。config 与 play_mode 每次 tick 重读，
    /// 因此设置/删除带来的变化无需重启定时器即可生效。
    fn spawn(self, app: &tauri::AppHandle) -> JoinHandle<()> {
        let app = app.clone();
        let mid = self.monitor_id;

        tokio::spawn(async move {
            // 从 AppHandle 按需获取共享资源
            let ctx = app.state::<AppContext>();
            let app_ctx: &AppContext = ctx.inner();
            let db = app_ctx.db.clone();

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

                // 不用启动时缓存的值：快捷键切图、删除联动都会改 DB，
                // 必须按最新 wallpaper_id / play_mode 计算下一张才不会回退到旧图。
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
                        // 架构重整 Phase 3：DB 写入（本屏 + 同步模式下的从屏）收进单事务，
                        // commit 后由 Manager 一次 reconcile 统一下发，不再逐条改 DB + 手动通知。
                        let is_sync = display_mode == "mirror" || display_mode == "extend";

                        let txn = match db.begin().await {
                            Ok(t) => t,
                            Err(e) => {
                                error!("[Carousel] Failed to begin transaction: {}", e);
                                continue;
                            }
                        };
                        if let Err(e) =
                            monitor_config_service::update_wallpaper_id(&txn, &mid, new_wid).await
                        {
                            error!(
                                "[Carousel] Failed to update wallpaper_id for {}: {}",
                                mid, e
                            );
                            continue;
                        }
                        if is_sync {
                            if let Ok(all_configs) = monitor_config_service::get_all(&txn).await {
                                for config in &all_configs {
                                    if config.active && config.monitor_id != mid {
                                        if let Err(e) = monitor_config_service::update_wallpaper_id(
                                            &txn,
                                            &config.monitor_id,
                                            new_wid,
                                        )
                                        .await
                                        {
                                            error!(
                                                "[Carousel] Failed to sync wallpaper_id for {}: {}",
                                                config.monitor_id, e
                                            );
                                            continue;
                                        }
                                    }
                                }
                            }
                        }
                        if let Err(e) = txn.commit().await {
                            error!("[Carousel] Failed to commit wallpaper advance: {}", e);
                            continue;
                        }

                        let scope = if is_sync {
                            ReconcileScope::All
                        } else {
                            ReconcileScope::One(mid.clone())
                        };
                        let mut manager = app_ctx.unified_manager.lock().await;
                        if let Err(e) = manager.reconcile(scope).await {
                            error!("[Carousel] Reconcile failed for {}: {}", mid, e);
                        }
                    }
                    Ok(None) => {
                        // 收藏夹为空或只有一张，无需切换
                    }
                    Err(e) => {
                        error!("[Carousel] Error getting next wallpaper for {}: {}", mid, e);
                    }
                }
            }
        })
    }
}
