//! 壁纸窗口管理相关命令

use tauri::State;

use crate::ctx::AppContext;
use crate::dto::wallpaper_window_dto::{
    CreateWallpaperWindowRequest, DestroyWallpaperWindowRequest,
};
use crate::dto::Validated;
use crate::platform::wallpaper_renderer::MonitorRect;
use crate::services::monitor_config_service;

use super::error::CommandResult;

/// 为指定显示器创建壁纸窗口
#[tauri::command]
pub async fn create_wallpaper_window(
    ctx: State<'_, AppContext>,
    req: Validated<CreateWallpaperWindowRequest>,
) -> CommandResult<()> {
    let req = req.into_inner();
    let app_ctx: &AppContext = ctx.inner();

    let rect = MonitorRect {
        x: req.x,
        y: req.y,
        width: req.width as i32,
        height: req.height as i32,
    };

    // 从 config 取当前壁纸，交给协调器决定 backend 并准备渲染目标
    let wallpaper_id = monitor_config_service::get_by_monitor_id(&app_ctx.db, &req.monitor_id)
        .await?
        .and_then(|config| config.wallpaper_id);

    let mut coordinator = app_ctx.render_coordinator.lock().await;
    // 先登记几何再判断有没有壁纸：没有壁纸的屏也必须进入 extend 的 union，
    // 否则虚拟桌面画布会缺一块，接缝位置算错。
    coordinator.register_monitor(&req.monitor_id, rect);

    let Some(wallpaper_id) = wallpaper_id else {
        return Ok(());
    };
    coordinator
        .ensure_target(&req.monitor_id, rect, wallpaper_id)
        .await?;

    Ok(())
}

/// 销毁指定显示器的壁纸窗口
#[tauri::command]
pub async fn destroy_wallpaper_window(
    ctx: State<'_, AppContext>,
    req: Validated<DestroyWallpaperWindowRequest>,
) -> CommandResult<()> {
    let req = req.into_inner();
    let app_ctx: &AppContext = ctx.inner();
    let mut coordinator = app_ctx.render_coordinator.lock().await;
    coordinator.destroy_target(&req.monitor_id).await;
    Ok(())
}

/// 销毁所有壁纸窗口
#[tauri::command]
pub async fn destroy_all_wallpaper_windows(ctx: State<'_, AppContext>) -> CommandResult<()> {
    let app_ctx: &AppContext = ctx.inner();
    let mut coordinator = app_ctx.render_coordinator.lock().await;
    let mids = coordinator.active_ids().await;
    for mid in mids {
        coordinator.destroy_target(&mid).await;
    }
    Ok(())
}

/// 隐藏所有壁纸窗口（全屏暂停）
#[tauri::command]
pub async fn hide_wallpaper_windows(ctx: State<'_, AppContext>) -> CommandResult<()> {
    let app_ctx: &AppContext = ctx.inner();
    let coordinator = app_ctx.render_coordinator.lock().await;
    coordinator.set_paused(true).await;
    Ok(())
}

/// 显示所有壁纸窗口（全屏恢复）
#[tauri::command]
pub async fn show_wallpaper_windows(ctx: State<'_, AppContext>) -> CommandResult<()> {
    let app_ctx: &AppContext = ctx.inner();
    let coordinator = app_ctx.render_coordinator.lock().await;
    coordinator.set_paused(false).await;
    Ok(())
}

/// 获取当前已创建渲染目标的 monitor_id 列表（Native + WebView 合并）
#[tauri::command]
pub async fn get_active_wallpaper_windows(
    ctx: State<'_, AppContext>,
) -> CommandResult<Vec<String>> {
    let app_ctx: &AppContext = ctx.inner();
    let coordinator = app_ctx.render_coordinator.lock().await;
    Ok(coordinator.active_ids().await)
}
