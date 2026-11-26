//! 显示器配置命令
//!
//! 把前端的配置增删改请求翻译成两件事：轮播定时器的启停，以及样式 / 壁纸
//! 变更对渲染目标的更新。渲染侧统一经 `render_coordinator` 路由到 Native
//! 渲染目标或 WebView 窗口事件，命令层只描述「改了什么」，不感知 backend。

use std::sync::Arc;

use tauri::State;
use tokio::sync::Mutex;

use crate::ctx::unified_manager::ReconcileScope;
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

/// 创建或更新显示器配置（同步组语义由后端裁决）
///
/// 前端永远只传单个 monitor_id；本命令读 display_mode 决定是否扩散到整组：
/// - mirror/extend：单事务把同一份配置全量对齐到所有 active 显示器，一次 reconcile(All)
/// - independent：只写请求里的那一屏，一次 reconcile(One)
///
/// 一次事务 + 一次 reconcile 是硬约束：若像旧前端那样并发 N 条 command，每条各自
/// reconcile 会看到「部分屏已更新」的中间态，且同一视频被重复 attach 导致帧时钟错开。
#[tauri::command]
pub async fn upsert_monitor_config(
    ctx: State<'_, AppContext>,
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
    req: Validated<UpsertMonitorConfigRequest>,
) -> CommandResult<monitor_config::Model> {
    let req: UpsertMonitorConfigRequest = req.into_inner();

    // play_interval / collection_id 变更需重建定时器 interval 或重算收藏夹
    let need_restart = req.play_interval.is_some() || req.collection_id.is_some();
    let app_ctx: &AppContext = ctx.inner();

    let (updated, is_sync) = monitor_config_service::upsert_group(&app_ctx.db, &req).await?;

    // ===== 定时器管理 =====
    // 同步组只保留基准屏一个定时器：组内所有屏共享同一壁纸，若每屏各启一个，
    // 每个 timer 都会触发一次整组切换，切换频率会翻 N 倍（与 start_all_carousel_timers
    // 的「同步组只启主屏」语义一致）。独立模式则各管各的。
    let scheduler_handle: Arc<Mutex<Scheduler>> = scheduler.inner().clone();
    let mut sched = scheduler_handle.lock().await;
    for config in &updated {
        if is_sync && config.monitor_id != req.monitor_id {
            sched.stop(&carousel_key(&config.monitor_id));
            continue;
        }
        sched.manage_carousel_timer(config, need_restart).await;
    }
    drop(sched);

    // ===== 唯一的渲染副作用入口：一次 action → 一次 reconcile =====
    let scope = if is_sync {
        ReconcileScope::All
    } else {
        ReconcileScope::One(req.monitor_id.clone())
    };
    let mut manager = app_ctx.unified_manager.lock().await;
    manager.reconcile(scope).await?;

    // 返回基准屏配置；组内其它屏由前端 fetchConfigs 全量刷新
    updated
        .into_iter()
        .find(|c| c.monitor_id == req.monitor_id)
        .ok_or_else(|| anyhow::anyhow!("基准屏配置未返回").into())
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

/// 拉取当前显示器的 extend 视口（壁纸页挂载时主动调用一次）
///
/// Rust 按目标屏真实 DPI 把物理坐标换算成 CSS 像素；前端零计算。
/// 后续几何变化由 `extend-viewport` 事件定向推送。
#[tauri::command]
pub async fn get_extend_viewport(
    ctx: State<'_, AppContext>,
    monitor_id: String,
) -> CommandResult<Option<crate::events::ExtendViewportPayload>> {
    let app_ctx: &AppContext = ctx.inner();
    let manager = app_ctx.unified_manager.lock().await;
    Ok(manager.current_extend_viewport(&monitor_id))
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
