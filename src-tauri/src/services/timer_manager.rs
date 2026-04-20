use std::collections::HashMap;
use std::sync::Arc;

use sea_orm::DatabaseConnection;
use tauri::Emitter;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};

use crate::entities::monitor_config;
use crate::services::{collection_service, monitor_config_service};

/// 壁纸变更事件 payload（发送给前端）
#[derive(Clone, serde::Serialize)]
pub struct WallpaperChangedPayload {
    pub monitor_id: String,
    pub wallpaper_id: i32,
}

/// 每个 monitor 的定时器句柄
struct TimerEntry {
    handle: JoinHandle<()>,
}

/// 轮播定时器管理器
///
/// 按 monitor_id 管理独立的定时器，通过 Tauri managed state 注入。
/// 本结构仅负责定时器生命周期（启停调度），不直接操作数据库。
pub struct TimerManager {
    timers: HashMap<String, TimerEntry>,
}

impl TimerManager {
    pub fn new() -> Self {
        Self {
            timers: HashMap::new(),
        }
    }

    /// 启动某个 monitor 的轮播定时器
    ///
    /// 只需 monitor_id，内部通过 service 查询 config 获取 play_interval/play_mode/collection_id。
    pub fn start(
        &mut self,
        monitor_id: String,
        db: DatabaseConnection,
        app_handle: tauri::AppHandle,
    ) {
        // 如果已有运行中的定时器，先停止
        self.stop(&monitor_id);

        let mid = monitor_id.clone();

        let handle = tokio::spawn(async move {
            // 查询 config 获取轮播参数
            let config = match monitor_config_service::get_by_monitor_id(&db, &mid).await {
                Ok(Some(c)) => c,
                Ok(None) => {
                    eprintln!("[TimerManager] Config not found for {}, aborting start", mid);
                    return;
                }
                Err(e) => {
                    eprintln!("[TimerManager] Failed to get config for {}: {}", mid, e);
                    return;
                }
            };

            let collection_id = match config.collection_id {
                Some(cid) => cid,
                None => {
                    eprintln!("[TimerManager] No collection_id for {}, aborting start", mid);
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
                let current_wid = match monitor_config_service::get_by_monitor_id(&db, &mid).await
                {
                    Ok(Some(c)) => c.wallpaper_id,
                    Ok(None) => {
                        eprintln!("[TimerManager] Config not found for {}, stopping", mid);
                        break;
                    }
                    Err(e) => {
                        eprintln!("[TimerManager] Failed to get config for {}: {}", mid, e);
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
                            eprintln!(
                                "[TimerManager] Failed to update wallpaper_id for {}: {}",
                                mid, e
                            );
                            continue;
                        }

                        // 发送全局事件通知前端
                        let payload = WallpaperChangedPayload {
                            monitor_id: mid.clone(),
                            wallpaper_id: new_wid,
                        };
                        if let Err(e) = app_handle.emit("wallpaper-changed", &payload) {
                            eprintln!("[TimerManager] Failed to emit event: {}", e);
                        }
                    }
                    Ok(None) => {
                        // 收藏夹为空或只有一张，无需切换
                    }
                    Err(e) => {
                        eprintln!(
                            "[TimerManager] Error getting next wallpaper for {}: {}",
                            mid, e
                        );
                    }
                }
            }
        });

        self.timers.insert(monitor_id, TimerEntry { handle });
    }

    /// 停止某个 monitor 的定时器
    pub fn stop(&mut self, monitor_id: &str) {
        if let Some(entry) = self.timers.remove(monitor_id) {
            entry.handle.abort();
        }
    }

    /// 停止所有定时器
    pub fn stop_all(&mut self) {
        for (_, entry) in self.timers.drain() {
            entry.handle.abort();
        }
    }

    /// 重启某个 monitor 的定时器（停旧启新）
    pub fn restart(
        &mut self,
        monitor_id: String,
        db: DatabaseConnection,
        app_handle: tauri::AppHandle,
    ) {
        self.stop(&monitor_id);
        self.start(monitor_id, db, app_handle);
    }

    /// 检查某个 monitor 是否有运行中的定时器
    pub fn is_running(&self, monitor_id: &str) -> bool {
        self.timers.contains_key(monitor_id)
    }
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

/// 传入 TimerManager 的 managed state 类型别名
pub type TimerManagerState = Arc<Mutex<TimerManager>>;

/// 创建 TimerManager state
pub fn create_timer_manager() -> TimerManagerState {
    Arc::new(Mutex::new(TimerManager::new()))
}
