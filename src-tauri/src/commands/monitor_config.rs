use std::sync::Arc;

use tauri::State;
use tokio::sync::Mutex;

use crate::ctx::AppContext;
use crate::dto::monitor_config_dto::{
    DeleteMonitorConfigRequest, GetMonitorConfigRequest, UpsertMonitorConfigRequest,
};
use crate::dto::Validated;
use crate::entities::monitor_config;
use crate::runtime::tasks::carousel::carousel_key;
use crate::runtime::Scheduler;
use crate::services::monitor_config_service;

use super::error::CommandResult;

/// 获取所有显示器配置
#[tauri::command]
pub async fn get_monitor_configs(
    ctx: State<'_, AppContext>,
) -> CommandResult<Vec<monitor_config::Model>> {
    Ok(monitor_config_service::get_all(&ctx.db).await?)
}

/// 根据 monitor_id 获取配置
#[tauri::command]
pub async fn get_monitor_config(
    ctx: State<'_, AppContext>,
    req: Validated<GetMonitorConfigRequest>,
) -> CommandResult<Option<monitor_config::Model>> {
    let req = req.into_inner();
    Ok(monitor_config_service::get_by_monitor_id(&ctx.db, &req.monitor_id).await?)
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
) -> CommandResult<monitor_config::Model> {
    let req: UpsertMonitorConfigRequest = req.into_inner();

    // 提取变更字段（在 req 被消费前克隆）
    let fit_mode_changed = req.fit_mode.clone();
    let wallpaper_changed = req.wallpaper_id;
    // 需要重启定时器的场景：play_interval 变更（需重建 interval）、collection_id 变更（切换收藏夹）
    let need_restart = req.play_interval.is_some() || req.collection_id.is_some();

    let config = monitor_config_service::upsert(&ctx.db, &req).await?;

    // ===== 定时器管理 =====
    let mut sched = scheduler.lock().await;
    sched.manage_carousel_timer(&config, need_restart).await;

    // ===== 样式 / 壁纸变更通知壁纸窗口 =====
    let is_sync = sched.is_sync_mode().await;

    let wm = ctx.window_manager.lock().await;
    if let Some(fit_mode) = fit_mode_changed.as_deref() {
        wm.notify_fit_mode_update(&config.monitor_id, fit_mode, is_sync);
    }
    if let Some(wid) = wallpaper_changed {
        wm.notify_wallpaper_update(&config.monitor_id, wid, is_sync);
    }

    Ok(config)
}

/// 删除显示器配置
#[tauri::command]
pub async fn delete_monitor_config(
    ctx: State<'_, AppContext>,
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
    req: Validated<DeleteMonitorConfigRequest>,
) -> CommandResult<()> {
    let req = req.into_inner();

    // 先停止可能存在的定时器
    if let Some(mid) = &req.monitor_id {
        let mut sched = scheduler.lock().await;
        sched.stop(&carousel_key(mid));
    }

    monitor_config_service::delete(&ctx.db, req.id).await?;

    Ok(())
}

/// 启动所有满足轮播条件的定时器（应用启动时由前端调用）
///
/// 委托 Scheduler 的 start_all_carousel_timers 方法，
/// display_mode 感知逻辑已内聚在 Scheduler 中。
#[tauri::command]
pub async fn start_timers(
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
) -> CommandResult<()> {
    let mut sched = scheduler.lock().await;
    sched.start_all_carousel_timers().await;
    Ok(())
}
