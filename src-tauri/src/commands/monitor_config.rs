//! 显示器配置命令
//!
//! 把前端的配置增删改请求翻译成两件事：轮播定时器的启停，以及样式 / 壁纸
//! 变更对渲染目标的更新。渲染侧统一经 `render_coordinator` 路由到 Native
//! 渲染目标或 WebView 窗口事件，命令层只描述「改了什么」，不感知 backend。

use std::sync::Arc;

use tauri::State;
use tokio::sync::Mutex;

use crate::ctx::reconcile::ReconcileScope;
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
    let app_ctx: &AppContext = ctx.inner();
    Ok(monitor_config_service::get_all(&app_ctx.db).await?)
}

/// 根据 monitor_id 获取配置
#[tauri::command]
pub async fn get_monitor_config(
    ctx: State<'_, AppContext>,
    req: Validated<GetMonitorConfigRequest>,
) -> CommandResult<Option<monitor_config::Model>> {
    let req = req.into_inner();
    let app_ctx: &AppContext = ctx.inner();
    Ok(monitor_config_service::get_by_monitor_id(&app_ctx.db, &req.monitor_id).await?)
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

    // 需要重启定时器的场景：play_interval 变更（需重建 interval）、collection_id 变更（切换收藏夹）
    let need_restart = req.play_interval.is_some() || req.collection_id.is_some();
    let app_ctx: &AppContext = ctx.inner();

    let config = monitor_config_service::upsert(&app_ctx.db, &req).await?;

    // ===== 定时器管理（Phase 3 范围排除：timer 归 Scheduler，不进 reconcile） =====
    let scheduler_handle: Arc<Mutex<Scheduler>> = scheduler.inner().clone();
    let mut sched = scheduler_handle.lock().await;
    sched.manage_carousel_timer(&config, need_restart).await;
    drop(sched);

    // ===== 唯一的渲染副作用入口：一次 action → 一次 reconcile =====
    // 单条 upsert 只改一个 monitor，scope = One；manager 内部 Mutex 保证串行（C3）。
    let mut manager = app_ctx.unified_manager.lock().await;
    manager
        .reconcile(ReconcileScope::One(config.monitor_id.clone()))
        .await?;

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
        let scheduler_handle: Arc<Mutex<Scheduler>> = scheduler.inner().clone();
        let mut sched = scheduler_handle.lock().await;
        sched.stop(&carousel_key(mid));
    }

    let app_ctx: &AppContext = ctx.inner();
    monitor_config_service::delete(&app_ctx.db, req.id).await?;

    Ok(())
}

/// 启动所有满足轮播条件的定时器（应用启动时由前端调用）
///
/// 委托 Scheduler 的 start_all_carousel_timers 方法，
/// display_mode 感知逻辑已内聚在 Scheduler 中。
#[tauri::command]
pub async fn start_timers(scheduler: State<'_, Arc<Mutex<Scheduler>>>) -> CommandResult<()> {
    let scheduler_handle: Arc<Mutex<Scheduler>> = scheduler.inner().clone();
    let mut sched = scheduler_handle.lock().await;
    sched.start_all_carousel_timers().await;
    Ok(())
}
