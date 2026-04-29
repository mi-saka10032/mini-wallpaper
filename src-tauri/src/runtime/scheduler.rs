//! 运行时任务调度器
//!
//! 全局唯一的异步任务生命周期管理者，所有需要后台执行的任务
//! 均通过此调度器注册 / 停止 / 重启。
//!
//! ## 设计
//! - **`TaskSpawner`**：任务工厂 trait，实现者仅持有纯业务参数，
//!   `spawn` 接收 `&AppHandle` 由调度器注入，按需获取共享资源
//! - **`Scheduler`**：持有 `AppHandle` 的任务调度器，
//!   负责任务的注册 / 停止 / 重启等生命周期管理，
//!   同时承载定时器编排和设置副作用等联动逻辑

use std::collections::HashMap;
use std::sync::Arc;

use log::{info, warn};
use sea_orm::DatabaseConnection;
use tauri::Manager;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::ctx::AppContext;
use crate::ctx::window_manager::WallpaperWindowManager;
use crate::dto::app_setting_dto::{self, keys as setting_keys};
use crate::entities::monitor_config;
use crate::events::VolumeChangedPayload;
use crate::services::{app_setting_service, collection_service, monitor_config_service};

use super::carousel::{carousel_key, CarouselTask};
use super::fullscreen_detector::{FullscreenDetectionTask, FULLSCREEN_TIMER_KEY};

/// 任务工厂 trait
///
/// 业务模块（carousel、fullscreen_detector 等）实现此 trait，
/// struct 仅持有纯业务参数（如 `monitor_id`），
/// `spawn` 接收 `&AppHandle` 由调度器注入，按需获取 db / window_manager 等共享资源。
///
/// # 示例
/// ```ignore
/// struct MyTask {
///     monitor_id: String,
/// }
///
/// impl TaskSpawner for MyTask {
///     fn spawn(self, app: &tauri::AppHandle) -> JoinHandle<()> {
///         let app = app.clone();
///         tokio::spawn(async move {
///             let ctx = app.state::<AppContext>();
///             // 使用 self.monitor_id（业务参数）
///             // 使用 ctx.db / ctx.window_manager（按需获取）
///         })
///     }
/// }
///
/// // 调用方：无需传入 app_handle，Scheduler 自动注入
/// scheduler.spawn("my_task".into(), MyTask { monitor_id: "xxx".into() });
/// ```
pub trait TaskSpawner {
    /// 消费 self，接收调度器注入的 AppHandle，创建异步任务并返回 JoinHandle
    fn spawn(self, app: &tauri::AppHandle) -> JoinHandle<()>;
}

/// 运行时任务调度器
///
/// 持有 `AppHandle`，通过控制反转向 `TaskSpawner` 注入句柄，
/// 同时承载定时器编排和设置副作用等联动逻辑。
///
/// 作为独立的全局 state 注册到 Tauri，与 `AppContext` 平级，
/// 在 `ExitRequested` 时统一停止所有后台任务。
pub struct Scheduler {
    /// Tauri 应用句柄（注入给 TaskSpawner）
    app: tauri::AppHandle,
    /// key -> JoinHandle 映射
    tasks: HashMap<String, JoinHandle<()>>,
}

impl Scheduler {
    /// 构造 Scheduler，注入 AppHandle
    pub fn new(app: tauri::AppHandle) -> Self {
        Self {
            app,
            tasks: HashMap::new(),
        }
    }

    /// 从 AppHandle 获取 db 连接（clone 后脱离 self 生命周期）
    fn db(&self) -> DatabaseConnection {
        self.app.state::<AppContext>().db.clone()
    }

    /// 从 AppHandle 获取 window_manager（clone Arc 后脱离 self 生命周期）
    fn window_manager(&self) -> Arc<Mutex<WallpaperWindowManager>> {
        self.app.state::<AppContext>().window_manager.clone()
    }

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

    // ==================== 轮播定时器编排 ====================

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

        let is_sync_mode = app_setting_dto::is_sync_mode(&display_mode);
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

    // ==================== 设置副作用 ====================

    /// 根据 setting key 执行对应的副作用
    ///
    /// 每个需要"写入后立即生效"的 key 在此注册处理逻辑。
    /// 无副作用的 key（如 theme、language）仅写入 DB，前端自行响应。
    pub async fn apply_setting_side_effect(
        &mut self,
        key: &str,
        value: &str,
        monitor_id: Option<&str>,
    ) {
        match key {
            setting_keys::PAUSE_ON_FULLSCREEN => {
                let enabled = value == "true";
                if enabled {
                    if !self.is_running(FULLSCREEN_TIMER_KEY) {
                        self.spawn(
                            FULLSCREEN_TIMER_KEY.to_string(),
                            FullscreenDetectionTask,
                        );
                        info!("[Scheduler] 全屏检测已启动");
                    }
                } else {
                    self.stop(FULLSCREEN_TIMER_KEY);
                    info!("[Scheduler] 全屏检测已停止");
                }
            }
            setting_keys::GLOBAL_VOLUME => {
                let volume = value.parse::<u32>().unwrap_or(0).min(100);
                let wm = self.window_manager();
                let mgr = wm.lock().await;
                mgr.broadcast(&VolumeChangedPayload { volume: volume as f64 });
                info!("[Scheduler] 音量已更新: {}%", volume);
            }
            setting_keys::DISPLAY_MODE => {
                self.apply_display_mode_side_effect(value, monitor_id).await;
            }
            _ => {}
        }
    }

    /// display_mode 变更副作用
    ///
    /// 执行顺序：
    /// 1. 以 monitor_id 为基准，同步配置到所有从属 monitor（mirror/extend 模式）
    /// 2. 通知所有壁纸窗口切换渲染模式（wallpaper_manager 广播 display-mode-changed）
    /// 3. 管理定时器：主 monitor 保持/启动定时器，从属 monitor 停止定时器
    async fn apply_display_mode_side_effect(
        &mut self,
        display_mode: &str,
        monitor_id: Option<&str>,
    ) {
        info!("[Scheduler] display_mode 变更: {}", display_mode);

        let db = self.db();
        let wm = self.window_manager();

        let configs = match monitor_config_service::get_all(&db).await {
            Ok(c) => c,
            Err(e) => {
                warn!("[Scheduler] 获取 monitor configs 失败: {}", e);
                return;
            }
        };

        let active_configs: Vec<_> = configs.iter().filter(|c| c.active).collect();
        if active_configs.is_empty() {
            return;
        }

        match display_mode {
            "independent" => {
                // 通知所有壁纸窗口切换到 independent
                {
                    let mgr = wm.lock().await;
                    for config in &active_configs {
                        if let Err(e) =
                            mgr.notify_display_mode_changed(&config.monitor_id, "independent")
                        {
                            warn!(
                                "[Scheduler] 发送 display-mode-changed 失败 {}: {}",
                                config.monitor_id, e
                            );
                        }
                    }
                }

                // 恢复所有满足条件的定时器
                for config in &active_configs {
                    let key = carousel_key(&config.monitor_id);
                    if monitor_config_service::should_start_timer(config) {
                        if let Some(cid) = config.collection_id {
                            match collection_service::has_enough_wallpapers(&db, cid).await {
                                Ok(true) => {
                                    if !self.is_running(&key) {
                                        self.spawn(
                                            key,
                                            CarouselTask {
                                                monitor_id: config.monitor_id.clone(),
                                            },
                                        );
                                        info!(
                                            "[Scheduler] 恢复定时器: {}",
                                            config.monitor_id
                                        );
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            "mirror" | "extend" => {
                // 步骤 1: 以 monitor_id 为基准，同步配置到所有从属 monitor
                let primary_mid = monitor_id.unwrap_or_else(|| {
                    active_configs
                        .first()
                        .map(|c| c.monitor_id.as_str())
                        .unwrap_or("")
                });

                let primary_config =
                    active_configs.iter().find(|c| c.monitor_id == primary_mid);
                if let Some(source) = primary_config {
                    for config in &active_configs {
                        if config.monitor_id == primary_mid {
                            continue;
                        }
                        if let Err(e) = monitor_config_service::sync_config_from(
                            &db,
                            &config.monitor_id,
                            source,
                        )
                        .await
                        {
                            warn!(
                                "[Scheduler] 同步配置到 {} 失败: {}",
                                config.monitor_id, e
                            );
                        }
                    }
                }

                // 步骤 2: 通知所有壁纸窗口切换渲染模式
                {
                    let mgr = wm.lock().await;
                    for config in &active_configs {
                        if let Err(e) =
                            mgr.notify_display_mode_changed(&config.monitor_id, display_mode)
                        {
                            warn!(
                                "[Scheduler] 发送 display-mode-changed 失败 {}: {}",
                                config.monitor_id, e
                            );
                        }
                    }
                }

                // 步骤 3: 定时器管理 — 仅保留主 monitor 的定时器，停止所有从属定时器
                for config in &active_configs {
                    if config.monitor_id != primary_mid {
                        let key = carousel_key(&config.monitor_id);
                        if self.is_running(&key) {
                            self.stop(&key);
                            info!(
                                "[Scheduler] 停止从属定时器: {}",
                                config.monitor_id
                            );
                        }
                    }
                }

                // 检查主 monitor 是否具备启动定时器条件
                if let Some(source) = primary_config {
                    let key = carousel_key(&source.monitor_id);
                    if monitor_config_service::should_start_timer(source) {
                        if let Some(cid) = source.collection_id {
                            match collection_service::has_enough_wallpapers(&db, cid).await {
                                Ok(true) => {
                                    self.restart(
                                        key,
                                        CarouselTask {
                                            monitor_id: source.monitor_id.clone(),
                                        },
                                    );
                                    info!(
                                        "[Scheduler] 主定时器已启动: {}",
                                        source.monitor_id
                                    );
                                }
                                Ok(false) => {
                                    self.stop(&key);
                                }
                                Err(e) => {
                                    warn!("[Scheduler] 检查收藏夹失败: {}", e);
                                    self.stop(&key);
                                }
                            }
                        } else {
                            self.stop(&key);
                        }
                    } else {
                        self.stop(&key);
                    }
                }
            }
            _ => {}
        }
    }

    // ==================== 全屏检测 ====================

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

