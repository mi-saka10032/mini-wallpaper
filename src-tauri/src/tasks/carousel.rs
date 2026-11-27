//! 轮播定时器业务模块
//!
//! 本模块负责轮播切换的业务逻辑：
//! - `CarouselTask`：轮播任务定义，实现 `TaskSpawner` trait
//! - `carousel_key()`：生成轮播定时器的 scheduler key
//!
//! 定时器生命周期由 `TaskRegistry` 统一管理，
//! `CarouselTask` 仅持有纯业务参数 `monitor_id`，
//! `spawn` 接收调度器注入的 `AppHandle`，按需获取 db / window_manager 等共享资源。

use log::{error, warn};
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration, MissedTickBehavior};

use crate::application::playlist;
use crate::context::unified_manager::ReconcileScope;
use crate::context::AppContext;
use crate::dto::shortcut_dto::Direction;
use crate::services::monitor_config_service;
use crate::tasks::{TaskDeps, TaskSpawner};

/// 轮播定时器在 TaskRegistry 中的 key 前缀
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
    fn spawn(self, deps: TaskDeps) -> JoinHandle<()> {
        let mid = self.monitor_id;
        let ctx = deps.ctx;

        tokio::spawn(async move {
            // 依赖由注册表显式注入，任务体内零反查。
            let app_ctx: &AppContext = ctx.as_ref();
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

            if config.collection_id.is_none() {
                warn!("[Carousel] No collection_id for {}, aborting start", mid);
                return;
            }

            let seconds = config.play_interval.max(5) as u64; // 最小 5 秒

            let mut tick = interval(Duration::from_secs(seconds));
            // 系统休眠唤醒后不该把错过的轮播一次性补齐（默认 Burst 会连跳十几张）。
            tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
            // 第一次 tick 立即触发，跳过
            tick.tick().await;

            // 连续失败计数：避免 release 下按 interval 周期无限写 error.log。
            // 达到阈值就停止本 timer，之后由配置变更触发 manage_carousel_timer 重建。
            const MAX_CONSECUTIVE_FAILURES: u32 = 3;
            let mut consecutive_failures: u32 = 0;

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
                // 收藏夹绑定也按最新 config：切回单张壁纸或收藏夹被删时直接退出，
                // 不再依赖 command 侧的 timer 重启兜底。
                if current_config.collection_id.is_none() {
                    break;
                }

                // 统一用例按 play_mode 分流：顺序走游标，随机走环形牌堆。
                // 牌堆以「命中集快照 + 当前显示 id」自愈，条件写入失败
                // （Ok(None)）时下一 tick 会自动以最新 id 为锚重洗。
                match playlist::pick_adjacent(app_ctx, &db, &current_config, &Direction::Next).await
                {
                    Ok(Some(new_wid)) => {
                        // 条件写入：读 config 到写回之间用户可能手动换过壁纸，
                        // 不匹配就放弃本轮，绝不覆盖用户刚选的那张。
                        match monitor_config_service::try_apply_group_wallpaper(
                            &db,
                            &mid,
                            Some(new_wid),
                            current_wid,
                        )
                        .await
                        {
                            Ok(None) => {
                                // 用户在这几毫秒内手动换了壁纸，放弃本轮。
                                consecutive_failures = 0;
                                continue;
                            }
                            Ok(Some(is_sync)) => {
                                consecutive_failures = 0;
                                // 轮播 tick 在 tasks 层没有 AppEffects 上下文，直接调 service；
                                // 与本任务直接调 monitor_config_service 的形态一致。
                                if let Err(e) =
                                    crate::services::wallpaper_service::increment_play_count(
                                        &db, new_wid,
                                    )
                                    .await
                                {
                                    error!("[Carousel] Failed to record play count: {}", e);
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
                            Err(e) => {
                                consecutive_failures += 1;
                                error!("[Carousel] Failed to apply group wallpaper: {}", e);
                            }
                        }
                    }
                    Ok(None) => {
                        // 收藏夹为空或只有一张，不算失败
                        consecutive_failures = 0;
                    }
                    Err(e) => {
                        consecutive_failures += 1;
                        error!("[Carousel] Error getting next wallpaper for {}: {}", mid, e);
                    }
                }

                if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    error!(
                        "[Carousel] {} 连续 {} 次轮播失败，停止该定时器；配置变更后会自动重建",
                        mid, MAX_CONSECUTIVE_FAILURES
                    );
                    break;
                }
            }
        })
    }
}
