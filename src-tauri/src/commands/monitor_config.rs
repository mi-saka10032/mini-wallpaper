//! 显示器配置命令
//!
//! 把前端的配置增删改请求翻译成两件事：轮播定时器的启停，以及样式 / 壁纸
//! 变更对渲染目标的更新。渲染侧统一经 `render_coordinator` 路由到 Native
//! 渲染目标或 WebView 窗口事件，命令层只描述「改了什么」，不感知 backend。

use std::sync::Arc;

use tauri::State;
use tokio::sync::Mutex;

use crate::application::{self, effects::AppEffects, AppServices};
use crate::context::AppContext;
use crate::dto::monitor_config_dto::{
    DeleteMonitorConfigRequest, GetMonitorConfigRequest, UpsertMonitorConfigRequest,
};
use crate::dto::Validated;
use crate::entities::monitor_config;
use crate::services::monitor_config_service;
use crate::tasks::carousel::carousel_key;
use crate::tasks::registry::TaskRegistry;

use super::error::CommandResult;

/// 获取所有显示器配置
#[tauri::command]
pub async fn get_monitor_configs(
    ctx: State<'_, Arc<AppContext>>,
) -> CommandResult<Vec<monitor_config::Model>> {
    let app_ctx: &AppContext = ctx.inner();
    Ok(monitor_config_service::get_all(&app_ctx.db).await?)
}

/// 根据 monitor_id 获取配置
#[tauri::command]
pub async fn get_monitor_config(
    ctx: State<'_, Arc<AppContext>>,
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
    ctx: State<'_, Arc<AppContext>>,
    scheduler: State<'_, Arc<Mutex<TaskRegistry>>>,
    req: Validated<UpsertMonitorConfigRequest>,
) -> CommandResult<monitor_config::Model> {
    let svc = AppServices::new(ctx.inner(), scheduler.inner());
    Ok(application::monitor::upsert_monitor_config(&svc, &req.into_inner()).await?)
}

/// 删除显示器配置
#[tauri::command]
pub async fn delete_monitor_config(
    ctx: State<'_, Arc<AppContext>>,
    scheduler: State<'_, Arc<Mutex<TaskRegistry>>>,
    req: Validated<DeleteMonitorConfigRequest>,
) -> CommandResult<()> {
    let req = req.into_inner();

    // 先停止可能存在的定时器；配置删除后牌堆条目也应随之消失，
    // 否则显示器热插拔会让 map 只增不删。
    if let Some(mid) = &req.monitor_id {
        let key = carousel_key(mid);
        let scheduler_handle: Arc<Mutex<TaskRegistry>> = scheduler.inner().clone();
        let mut sched = scheduler_handle.lock().await;
        sched.stop(&key);
        drop(sched);
        ctx.inner().play_queues().remove(&key);
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
    ctx: State<'_, Arc<AppContext>>,
    monitor_id: String,
) -> CommandResult<Option<crate::events::ExtendViewportPayload>> {
    let app_ctx: &AppContext = ctx.inner();
    let manager = app_ctx.unified_manager.lock().await;
    Ok(manager.current_extend_viewport(&monitor_id))
}

/// 启动所有满足轮播条件的定时器（应用启动时由前端调用）
///
/// 委托 TaskRegistry 的 start_all_carousel_timers 方法，
/// display_mode 感知逻辑已内聚在 TaskRegistry 中。
#[tauri::command]
pub async fn start_timers(
    ctx: State<'_, Arc<AppContext>>,
    scheduler: State<'_, Arc<Mutex<TaskRegistry>>>,
) -> CommandResult<()> {
    let svc = AppServices::new(ctx.inner(), scheduler.inner());
    let mut guard = svc.scheduler.lock().await;
    AppEffects::new(svc.ctx, &mut guard)
        .start_all_carousel_timers()
        .await;
    Ok(())
}
