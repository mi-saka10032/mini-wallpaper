use std::sync::Arc;

use tauri::State;
use tokio::sync::Mutex;

use crate::ctx::AppContext;
use crate::dto::monitor_config_dto::{
    DeleteMonitorConfigRequest, GetMonitorConfigRequest, UpsertMonitorConfigRequest,
};
use crate::dto::Validated;
use crate::entities::monitor_config;
use crate::runtime::carousel::{
    carousel_key, collection_has_enough_wallpapers, should_start_timer, CarouselTask,
};
use crate::runtime::Scheduler;
use crate::services::{app_setting_service, monitor_config_service};

use log::info;

/// 获取所有显示器配置
#[tauri::command]
pub async fn get_monitor_configs(
    ctx: State<'_, AppContext>,
) -> Result<Vec<monitor_config::Model>, String> {
    monitor_config_service::get_all(&ctx.db)
        .await
        .map_err(|e| e.to_string())
}

/// 根据 monitor_id 获取配置
#[tauri::command]
pub async fn get_monitor_config(
    ctx: State<'_, AppContext>,
    req: Validated<GetMonitorConfigRequest>,
) -> Result<Option<monitor_config::Model>, String> {
    let req = req.into_inner();
    monitor_config_service::get_by_monitor_id(&ctx.db, &req.monitor_id)
        .await
        .map_err(|e| e.to_string())
}

/// 创建或更新显示器配置
///
/// upsert 后自动管理定时器：
/// - 满足轮播条件 → 通过 Scheduler 启动/重启定时器
/// - 不满足 → 通过 Scheduler 停止定时器
///
/// 同时检测 fitMode / displayMode 变更，向壁纸窗口发送 config-changed 事件，
/// 使样式修改即时生效，无需重新加载壁纸。
#[tauri::command]
pub async fn upsert_monitor_config(
    ctx: State<'_, AppContext>,
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
    req: Validated<UpsertMonitorConfigRequest>,
) -> Result<monitor_config::Model, String> {
    let req: UpsertMonitorConfigRequest = req.into_inner();

    // 提取变更字段（在 req 被消费前克隆）
    let fit_mode_changed = req.fit_mode.clone();
    let wallpaper_changed = req.wallpaper_id;
    // 需要重启定时器的场景：play_interval 变更（需重建 interval）、collection_id 变更（切换收藏夹）
    let need_restart = req.play_interval.is_some() || req.collection_id.is_some();

    let config = monitor_config_service::upsert(&ctx.db, &req)
        .await
        .map_err(|e| e.to_string())?;

    // ===== 定时器管理 =====
    // should_start_timer 基于更新后的 config 判断 active && is_enabled
    manage_timer_for_config(&ctx, &scheduler, &config, need_restart).await;

    // ===== 样式 / 壁纸变更通知壁纸窗口 =====
    // 读取全局 display_mode，决定是否需要广播到所有窗口
    let display_mode = app_setting_service::get(&ctx.db, "display_mode")
        .await
        .unwrap_or(Some("independent".to_string()))
        .unwrap_or_else(|| "independent".to_string());

    let is_sync_mode = display_mode == "mirror" || display_mode == "extend";

    if is_sync_mode {
        // 同步模式：通知所有壁纸窗口
        let wm = ctx.window_manager.lock().await;
        let all_ids = wm.get_active_window_ids();
        for mid in &all_ids {
            if let Some(fit_mode) = fit_mode_changed.as_deref() {
                if let Err(e) = wm.notify_fit_mode_changed(mid, fit_mode) {
                    log::warn!("[upsert] 发送 fit-mode-changed 事件失败 {}: {}", mid, e);
                }
            }
            if let Some(wid) = wallpaper_changed {
                if let Err(e) = wm.update_window(mid, wid) {
                    log::warn!("[upsert] 壁纸窗口更新失败 {}: {}", mid, e);
                }
            }
        }
    } else {
        notify_window_changes(
            &ctx,
            &config.monitor_id,
            fit_mode_changed.as_deref(),
            wallpaper_changed,
        )
        .await;
    }

    Ok(config)
}

/// 通知壁纸窗口样式 / 壁纸变更
///
/// 根据传入的 Option 字段，按需向对应 monitor_id 的壁纸窗口发送事件：
/// - `fit_mode`   → fit-mode-changed
/// - `wallpaper_id` → wallpaper-changed（更新壁纸图片）
async fn notify_window_changes(
    ctx: &AppContext,
    monitor_id: &str,
    fit_mode: Option<&str>,
    wallpaper_id: Option<i32>,
) {
    // 两个字段都为 None 时无需获取锁
    if fit_mode.is_none() && wallpaper_id.is_none() {
        return;
    }

    let wm = ctx.window_manager.lock().await;

    if let Some(fit_mode) = fit_mode {
        if let Err(e) = wm.notify_fit_mode_changed(monitor_id, fit_mode) {
            log::warn!("[upsert] 发送 fit-mode-changed 事件失败: {}", e);
        }
    }
    if let Some(wallpaper_id) = wallpaper_id {
        if let Err(e) = wm.update_window(monitor_id, wallpaper_id) {
            log::warn!("[upsert] 壁纸窗口更新失败: {}", e);
        }
    }
}

/// 根据更新后的 config 状态管理定时器
///
/// - `need_restart`: play_interval / collection_id 变更时为 true，需要重启定时器
///
/// should_start_timer 基于更新后的 config 判断 active && is_enabled（总开关），
/// collection_id 是否存在由本函数额外判断。
async fn manage_timer_for_config(
    ctx: &AppContext,
    scheduler: &Arc<Mutex<Scheduler>>,
    config: &monitor_config::Model,
    need_restart: bool,
) {
    let key = carousel_key(&config.monitor_id);
    let mut sched = scheduler.lock().await;

    if !should_start_timer(config) {
        // active=false 或 is_enabled=false → 无条件停止
        sched.stop(&key);
        return;
    }

    // 以下 should_start_timer=true（active && is_enabled）

    let cid = match config.collection_id {
        Some(cid) => cid,
        None => {
            // 没有 collection_id，定时器不应运行
            sched.stop(&key);
            return;
        }
    };

    if need_restart {
        // play_interval 或 collection_id 变更 → 重启定时器
        match collection_has_enough_wallpapers(&ctx.db, cid).await {
            Ok(true) => {
                sched.restart(
                    key,
                    CarouselTask {
                        app: ctx.app_handle.clone(),
                        monitor_id: config.monitor_id.clone(),
                    },
                );
            }
            Ok(false) => {
                sched.stop(&key);
            }
            Err(e) => {
                log::warn!("[upsert] Failed to check collection wallpapers: {}", e);
                sched.stop(&key);
            }
        }
    } else if !sched.is_running(&key) {
        // 定时器未运行但条件满足（如 is_enabled 从 false→true）→ 启动
        match collection_has_enough_wallpapers(&ctx.db, cid).await {
            Ok(true) => {
                sched.spawn(
                    key,
                    CarouselTask {
                        app: ctx.app_handle.clone(),
                        monitor_id: config.monitor_id.clone(),
                    },
                );
            }
            Ok(false) => {}
            Err(e) => {
                log::warn!("[upsert] Failed to check collection wallpapers: {}", e);
            }
        }
    }
    // 定时器已在运行且无需重启 → 不做任何操作
}

/// 删除显示器配置
#[tauri::command]
pub async fn delete_monitor_config(
    ctx: State<'_, AppContext>,
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
    req: Validated<DeleteMonitorConfigRequest>,
) -> Result<(), String> {
    let req = req.into_inner();

    // 先停止可能存在的定时器
    if let Some(mid) = &req.monitor_id {
        let mut sched = scheduler.lock().await;
        sched.stop(&carousel_key(mid));
    }

    monitor_config_service::delete(&ctx.db, req.id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// 启动所有满足轮播条件的定时器（应用启动时由前端调用）
///
/// display_mode 感知：
/// - independent: 为每个满足条件的 monitor 启动独立定时器
/// - mirror/extend: 仅为第一个满足条件的 active monitor 启动定时器（主定时器）
#[tauri::command]
pub async fn start_timers(
    ctx: State<'_, AppContext>,
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
) -> Result<(), String> {
    let configs = monitor_config_service::get_all(&ctx.db)
        .await
        .map_err(|e| e.to_string())?;

    // 读取全局 display_mode
    let display_mode = app_setting_service::get(&ctx.db, "display_mode")
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "independent".to_string());

    let is_sync_mode = display_mode == "mirror" || display_mode == "extend";

    let mut sched = scheduler.lock().await;
    let mut primary_started = false;

    for config in &configs {
        if should_start_timer(config) {
            // mirror/extend 模式下，只启动第一个满足条件的定时器
            if is_sync_mode && primary_started {
                continue;
            }

            if let Some(cid) = config.collection_id {
                match collection_has_enough_wallpapers(&ctx.db, cid).await {
                    Ok(true) => {
                        let key = carousel_key(&config.monitor_id);
                        sched.spawn(
                            key.clone(),
                            CarouselTask {
                                app: ctx.app_handle.clone(),
                                monitor_id: config.monitor_id.clone(),
                            },
                        );
                        info!("[start_timers] 启动定时器: {} (display_mode={})", config.monitor_id, display_mode);

                        if is_sync_mode {
                            primary_started = true;
                        }
                    }
                    Ok(false) => {}
                    Err(e) => {
                        log::warn!("[start_timers] 检查收藏夹失败 {}: {}", config.monitor_id, e);
                    }
                }
            }
        }
    }

    Ok(())
}
