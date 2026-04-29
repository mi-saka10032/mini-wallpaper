//! 壁纸窗口管理相关命令

use tauri::State;

use crate::ctx::AppContext;
use crate::dto::wallpaper_window_dto::{CreateWallpaperWindowRequest, DestroyWallpaperWindowRequest};
use crate::dto::Validated;

use super::error::CommandResult;

/// 为指定显示器创建壁纸窗口
#[tauri::command]
pub async fn create_wallpaper_window(
    ctx: State<'_, AppContext>,
    req: Validated<CreateWallpaperWindowRequest>,
) -> CommandResult<()> {
    let req = req.into_inner();
    let mut mgr = ctx.window_manager.lock().await;
    mgr.create_window(&req.monitor_id, req.x, req.y, req.width, req.height, req.extra_query.as_deref())?;
    Ok(())
}

/// 销毁指定显示器的壁纸窗口
#[tauri::command]
pub async fn destroy_wallpaper_window(
    ctx: State<'_, AppContext>,
    req: Validated<DestroyWallpaperWindowRequest>,
) -> CommandResult<()> {
    let req = req.into_inner();
    let mut mgr = ctx.window_manager.lock().await;
    mgr.destroy_window(&req.monitor_id);
    Ok(())
}

/// 销毁所有壁纸窗口
#[tauri::command]
pub async fn destroy_all_wallpaper_windows(
    ctx: State<'_, AppContext>,
) -> CommandResult<()> {
    let mut mgr = ctx.window_manager.lock().await;
    mgr.destroy_all();
    Ok(())
}

/// 隐藏所有壁纸窗口
#[tauri::command]
pub async fn hide_wallpaper_windows(
    ctx: State<'_, AppContext>,
) -> CommandResult<()> {
    let mgr = ctx.window_manager.lock().await;
    mgr.hide_all();
    Ok(())
}

/// 显示所有壁纸窗口
#[tauri::command]
pub async fn show_wallpaper_windows(
    ctx: State<'_, AppContext>,
) -> CommandResult<()> {
    let mgr = ctx.window_manager.lock().await;
    mgr.show_all();
    Ok(())
}

/// 获取当前已创建壁纸窗口的 monitor_id 列表
#[tauri::command]
pub async fn get_active_wallpaper_windows(
    ctx: State<'_, AppContext>,
) -> CommandResult<Vec<String>> {
    let mgr = ctx.window_manager.lock().await;
    Ok(mgr.get_active_window_ids())
}
