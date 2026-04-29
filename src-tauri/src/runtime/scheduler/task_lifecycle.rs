//! 任务生命周期管理与编排
//!
//! 包含：
//! - **JoinHandle 生命周期管理**：spawn / register / stop / stop_all / is_running / restart
//! - **轮播/全屏检测编排**：start_all_carousel_timers / manage_carousel_timer / init_fullscreen_detection
//!
//! 所有方法通过 `impl Scheduler` 跨文件实现，
//! 天然拥有 `self.app`、`self.tasks` 等内部状态，零参数透传。

use log::{info, warn};
use tokio::task::JoinHandle;

use super::Scheduler;
use crate::dto::app_setting_dto::keys as setting_keys;
use crate::entities::monitor_config;
use crate::runtime::tasks::carousel::{carousel_key, CarouselTask};
use crate::runtime::tasks::fullscreen_detector::{FullscreenDetectionTask, FULLSCREEN_TIMER_KEY};
use crate::runtime::tasks::TaskSpawner;
use crate::services::{app_setting_service, collection_service, monitor_config_service};

// ==================== JoinHandle 生命周期管理 ====================

impl Scheduler {
    /// 通过 TaskSpawner trait 创建并注册任务
    ///
    /// 如果同 key 已存在运行中的任务，会先 abort 旧任务再注册新任务。
    pub fn spawn(&mut self, key: String, task: impl TaskSpawner) {
        let handle = task.spawn(&self.app);
        self.register(key, handle);
    }

    /// 注册一个已有的 JoinHandle（低层 API，优先使用 `spawn`）
    ///
    /// 如果同 key 已存在运行中的任务，会先 abort 旧任务再注册新任务。
    pub fn register(&mut self, key: String, handle: JoinHandle<()>) {
        if let Some(old) = self.tasks.remove(&key) {
            old.abort();
            info!("[Scheduler] 已停止旧任务: {}", key);
        }
        self.tasks.insert(key.clone(), handle);
        info!("[Scheduler] 已注册任务: {}", key);
    }

    /// 停止指定 key 的任务
    pub fn stop(&mut self, key: &str) {
        if let Some(handle) = self.tasks.remove(key) {
            handle.abort();
            info!("[Scheduler] 已停止任务: {}", key);
        }
    }

    /// 停止所有任务
    pub fn stop_all(&mut self) {
        let count = self.tasks.len();
        for (key, handle) in self.tasks.drain() {
            handle.abort();
            info!("[Scheduler] 已停止任务: {}", key);
        }
        info!("[Scheduler] 已停止全部 {} 个任务", count);
    }

    /// 检查指定 key 是否有运行中的任务
    pub fn is_running(&self, key: &str) -> bool {
        self.tasks
            .get(key)
            .map(|h| !h.is_finished())
            .unwrap_or(false)
    }

    /// 通过 TaskSpawner 重启任务
    ///
    /// 语义上等同于 `spawn`（内部已处理旧任务），更明确地表达"重启"意图。
    pub fn restart(&mut self, key: String, task: impl TaskSpawner) {
        self.spawn(key, task);
    }

    // ==================== 轮播 / 全屏检测编排 ====================

    /// 根据更新后的 config 状态管理轮播定时器
    ///
    /// - `need_restart`: play_interval / collection_id 变更时为 true，需要重启定时器
    ///
    /// 基于 config 判断 active && is_enabled（总开关），
    /// collection_id 是否存在由本函数额外判断。
    pub async fn manage_carousel_timer(
        &mut self,
        config: &monitor_config::Model,
        need_restart: bool,
    ) {
        let key = carousel_key(&config.monitor_id);
        let db = self.db();

        if !monitor_config_service::should_start_timer(config) {
            self.stop(&key);
            return;
        }

        let cid = match config.collection_id {
            Some(cid) => cid,
            None => {
                self.stop(&key);
                return;
            }
        };

        if need_restart {
            match collection_service::has_enough_wallpapers(&db, cid).await {
                Ok(true) => {
                    self.restart(
                        key,
                        CarouselTask { monitor_id: config.monitor_id.clone() },
                    );
                }
                Ok(false) => {
                    self.stop(&key);
                }
                Err(e) => {
                    warn!("[Scheduler] Failed to check collection wallpapers: {}", e);
                    self.stop(&key);
                }
            }
        } else if !self.is_running(&key) {
            match collection_service::has_enough_wallpapers(&db, cid).await {
                Ok(true) => {
                    self.spawn(
                        key,
                        CarouselTask { monitor_id: config.monitor_id.clone() },
                    );
                }
                Ok(false) => {}
                Err(e) => {
                    warn!("[Scheduler] Failed to check collection wallpapers: {}", e);
                }
            }
        }
    }

    /// 启动所有满足轮播条件的定时器（应用启动时调用）
    ///
    /// display_mode 感知：
    /// - independent: 为每个满足条件的 monitor 启动独立定时器
    /// - mirror/extend: 仅为第一个满足条件的 active monitor 启动定时器
    pub async fn start_all_carousel_timers(&mut self) {
        let db = self.db();

        let configs = match monitor_config_service::get_all(&db).await {
            Ok(c) => c,
            Err(e) => {
                warn!("[Scheduler] 获取 monitor configs 失败: {}", e);
                return;
            }
        };

        let display_mode = app_setting_service::get(&db, setting_keys::DISPLAY_MODE)
            .await
            .unwrap_or(None)
            .unwrap_or_else(|| "independent".to_string());

        let is_sync_mode = display_mode == "mirror" || display_mode == "extend";
        let mut primary_started = false;

        for config in &configs {
            if monitor_config_service::should_start_timer(config) {
                if is_sync_mode && primary_started {
                    continue;
                }

                if let Some(cid) = config.collection_id {
                    match collection_service::has_enough_wallpapers(&db, cid).await {
                        Ok(true) => {
                            let key = carousel_key(&config.monitor_id);
                            self.spawn(
                                key,
                                CarouselTask { monitor_id: config.monitor_id.clone() },
                            );
                            info!(
                                "[Scheduler] 启动定时器: {} (display_mode={})",
                                config.monitor_id, display_mode
                            );

                            if is_sync_mode {
                                primary_started = true;
                            }
                        }
                        Ok(false) => {}
                        Err(e) => {
                            warn!(
                                "[Scheduler] 检查收藏夹失败 {}: {}",
                                config.monitor_id, e
                            );
                        }
                    }
                }
            }
        }
    }

    /// 初始化全屏检测（读取 DB 设置，按需启动）
    pub async fn init_fullscreen_detection(&mut self) {
        let db = self.db();

        let should_start = app_setting_service::get(&db, setting_keys::PAUSE_ON_FULLSCREEN)
            .await
            .unwrap_or(None)
            .map(|v| v == "true")
            .unwrap_or(false);

        if should_start && !self.is_running(FULLSCREEN_TIMER_KEY) {
            self.spawn(FULLSCREEN_TIMER_KEY.to_string(), FullscreenDetectionTask);
        }
    }
}
