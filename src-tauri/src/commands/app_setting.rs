use std::collections::HashMap;
use std::sync::Arc;

use log::{info, warn};
use tauri::State;
use tokio::sync::Mutex;

use crate::ctx::AppContext;
use crate::runtime::carousel::{
    carousel_key, collection_has_enough_wallpapers, should_start_timer, CarouselTask,
};
use crate::runtime::fullscreen_detector::{FullscreenDetectionTask, FULLSCREEN_TIMER_KEY};
use crate::runtime::Scheduler;
use crate::dto::app_setting_dto::{GetSettingRequest, SetSettingRequest};
use crate::dto::Validated;
use crate::services::{app_setting_service, monitor_config_service};

/// 已知的 setting key 常量（与前端 SETTING_KEYS 保持一致）
pub(crate) mod keys {
    pub const THEME: &str = "theme";
    pub const LANGUAGE: &str = "language";
    pub const CLOSE_TO_TRAY: &str = "close_to_tray";
    pub const PAUSE_ON_FULLSCREEN: &str = "pause_on_fullscreen";
    pub const GLOBAL_VOLUME: &str = "global_volume";
    pub const DISPLAY_MODE: &str = "display_mode";
    pub const SHORTCUT_NEXT_WALLPAPER: &str = "shortcut_next_wallpaper";
    pub const SHORTCUT_PREV_WALLPAPER: &str = "shortcut_prev_wallpaper";
}

/// 获取所有设置（返回 key-value 对象）
#[tauri::command]
pub async fn get_settings(
    ctx: State<'_, AppContext>,
) -> Result<HashMap<String, String>, String> {
    let settings = app_setting_service::get_all(&ctx.db)
        .await
        .map_err(|e| e.to_string())?;

    let map: HashMap<String, String> = settings
        .into_iter()
        .map(|s| (s.key, s.value))
        .collect();

    Ok(map)
}

/// 获取单个设置值
#[tauri::command]
pub async fn get_setting(
    ctx: State<'_, AppContext>,
    req: Validated<GetSettingRequest>,
) -> Result<Option<String>, String> {
    let req = req.into_inner();
    app_setting_service::get(&ctx.db, &req.key)
        .await
        .map_err(|e| e.to_string())
}

/// 设置键值对（写入 DB + 按 key 触发副作用）
///
/// 统一入口：前端所有 setting 变更都通过此 command，
/// 内部通过 match key 模式，在写入 DB 后立即执行对应的副作用，
/// 确保设置变更即时生效。
///
/// `monitor_id`: 可选参数，display_mode 变更时需要传入当前选中的显示器 ID，
/// 用于确定"基准显示器"以同步配置到其他显示器。
#[tauri::command]
pub async fn set_setting(
    ctx: State<'_, AppContext>,
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
    req: Validated<SetSettingRequest>,
    monitor_id: Option<String>,
) -> Result<(), String> {
    let req = req.into_inner();

    // 跨字段校验：按 key 校验 value 格式
    req.validate_value_format()?;

    // 1. 写入 DB
    app_setting_service::set(&ctx.db, &req.key, &req.value)
        .await
        .map_err(|e| e.to_string())?;

    // 2. 按 key 执行副作用
    apply_setting_side_effect(&req.key, &req.value, &ctx, &scheduler, monitor_id.as_deref()).await;

    Ok(())
}

/// 音量变更事件 payload（广播给所有壁纸窗口）
#[derive(Clone, serde::Serialize)]
struct VolumeChangedPayload {
    /// 音量值 0-100
    volume: u32,
}

/// 根据 setting key 执行对应的副作用
///
/// 每个需要"写入后立即生效"的 key 在此注册处理逻辑。
/// 无副作用的 key（如 theme、language）仅写入 DB，前端自行响应。
async fn apply_setting_side_effect(
    key: &str,
    value: &str,
    ctx: &AppContext,
    scheduler: &Arc<Mutex<Scheduler>>,
    monitor_id: Option<&str>,
) {
    match key {
        keys::PAUSE_ON_FULLSCREEN => {
            let enabled = value == "true";
            let mut sched = scheduler.lock().await;
            if enabled {
                if !sched.is_running(FULLSCREEN_TIMER_KEY) {
                    sched.spawn(
                        FULLSCREEN_TIMER_KEY.to_string(),
                        FullscreenDetectionTask { app: ctx.app_handle.clone() },
                    );
                    info!("[Setting] 全屏检测已启动");
                }
            } else {
                sched.stop(FULLSCREEN_TIMER_KEY);
                info!("[Setting] 全屏检测已停止");
            }
        }
        keys::GLOBAL_VOLUME => {
            // DTO 层已校验 value 为 0~100 的合法整数，此处 parse 安全
            let volume = value.parse::<u32>().unwrap_or(0).min(100);
            let mgr = ctx.window_manager.lock().await;
            mgr.broadcast("volume-changed", &VolumeChangedPayload { volume });
            info!("[Setting] 音量已更新: {}%", volume);
        }
        keys::DISPLAY_MODE => {
            apply_display_mode_side_effect(value, ctx, scheduler, monitor_id).await;
        }
        _ => {
            // 无副作用的 key，仅写入 DB 即可
        }
    }
}

/// display_mode 变更副作用
///
/// 执行顺序：
/// 1. 以 monitor_id 为基准，同步配置到所有从属 monitor（mirror/extend 模式）
/// 2. 通知所有壁纸窗口切换渲染模式（wallpaper_manager 广播 display-mode-changed）
/// 3. 管理定时器：主 monitor 保持/启动定时器，从属 monitor 停止定时器
async fn apply_display_mode_side_effect(
    display_mode: &str,
    ctx: &AppContext,
    scheduler: &Arc<Mutex<Scheduler>>,
    monitor_id: Option<&str>,
) {
    info!("[Setting] display_mode 变更: {}", display_mode);

    // 获取所有 active 的 monitor configs
    let configs = match monitor_config_service::get_all(&ctx.db).await {
        Ok(c) => c,
        Err(e) => {
            warn!("[Setting] 获取 monitor configs 失败: {}", e);
            return;
        }
    };

    let active_configs: Vec<_> = configs.iter().filter(|c| c.active).collect();
    if active_configs.is_empty() {
        return;
    }

    match display_mode {
        "independent" => {
            // 独立模式：通知所有壁纸窗口切换到 independent，恢复各自独立的定时器
            let wm = ctx.window_manager.lock().await;
            for config in &active_configs {
                if let Err(e) = wm.notify_display_mode_changed(&config.monitor_id, "independent") {
                    warn!("[Setting] 发送 display-mode-changed 失败 {}: {}", config.monitor_id, e);
                }
            }
            drop(wm);

            // 恢复所有满足条件的定时器
            let mut sched = scheduler.lock().await;
            for config in &active_configs {
                let key = carousel_key(&config.monitor_id);
                if should_start_timer(config) {
                    if let Some(cid) = config.collection_id {
                        match collection_has_enough_wallpapers(&ctx.db, cid).await {
                            Ok(true) => {
                                if !sched.is_running(&key) {
                                    sched.spawn(
                                        key,
                                        CarouselTask {
                                            app: ctx.app_handle.clone(),
                                            monitor_id: config.monitor_id.clone(),
                                        },
                                    );
                                    info!("[Setting] 恢复定时器: {}", config.monitor_id);
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
                active_configs.first().map(|c| c.monitor_id.as_str()).unwrap_or("")
            });

            let primary_config = active_configs.iter().find(|c| c.monitor_id == primary_mid);
            if let Some(source) = primary_config {
                // 同步除 id/monitor_id 外的全部配置到其他 active monitor
                for config in &active_configs {
                    if config.monitor_id == primary_mid {
                        continue;
                    }
                    if let Err(e) = monitor_config_service::sync_config_from(
                        &ctx.db,
                        &config.monitor_id,
                        source,
                    ).await {
                        warn!("[Setting] 同步配置到 {} 失败: {}", config.monitor_id, e);
                    }
                }
            }

            // 步骤 2: 通知所有壁纸窗口切换渲染模式
            let wm = ctx.window_manager.lock().await;
            for config in &active_configs {
                if let Err(e) = wm.notify_display_mode_changed(&config.monitor_id, display_mode) {
                    warn!("[Setting] 发送 display-mode-changed 失败 {}: {}", config.monitor_id, e);
                }
            }
            drop(wm);

            // 步骤 3: 定时器管理 — 仅保留主 monitor 的定时器，停止所有从属定时器
            let mut sched = scheduler.lock().await;

            // 先停止所有从属 monitor 的定时器
            for config in &active_configs {
                if config.monitor_id != primary_mid {
                    let key = carousel_key(&config.monitor_id);
                    if sched.is_running(&key) {
                        sched.stop(&key);
                        info!("[Setting] 停止从属定时器: {}", config.monitor_id);
                    }
                }
            }

            // 检查主 monitor 是否具备启动定时器条件
            if let Some(source) = primary_config {
                let key = carousel_key(&source.monitor_id);
                if should_start_timer(source) {
                    if let Some(cid) = source.collection_id {
                        match collection_has_enough_wallpapers(&ctx.db, cid).await {
                            Ok(true) => {
                                // 重启主定时器（确保使用最新配置）
                                sched.restart(
                                    key,
                                    CarouselTask {
                                        app: ctx.app_handle.clone(),
                                        monitor_id: source.monitor_id.clone(),
                                    },
                                );
                                info!("[Setting] 主定时器已启动: {}", source.monitor_id);
                            }
                            Ok(false) => {
                                sched.stop(&key);
                            }
                            Err(e) => {
                                warn!("[Setting] 检查收藏夹失败: {}", e);
                                sched.stop(&key);
                            }
                        }
                    } else {
                        sched.stop(&key);
                    }
                } else {
                    sched.stop(&key);
                }
            }
        }
        _ => {}
    }
}