use std::sync::Arc;

use tauri::State;
use tokio::sync::Mutex;

use crate::ctx::AppContext;
use crate::dto::monitor_config_dto::{
    DeleteMonitorConfigRequest, GetMonitorConfigRequest, UpsertMonitorConfigRequest,
};
use crate::dto::Validated;
use crate::entities::monitor_config;
use crate::runtime::carousel::carousel_key;
use crate::runtime::Scheduler;
use crate::services::{app_setting_service, monitor_config_service};

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
    {
        let mut sched = scheduler.lock().await;
        sched.manage_carousel_timer(&config, need_restart).await;
    }

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
/// 委托 Scheduler 的 start_all_carousel_timers 方法，
/// display_mode 感知逻辑已内聚在 Scheduler 中。
#[tauri::command]
pub async fn start_timers(
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
) -> Result<(), String> {
    let mut sched = scheduler.lock().await;
    sched.start_all_carousel_timers().await;
    Ok(())
}
