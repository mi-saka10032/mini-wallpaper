//! 壁纸窗口管理相关命令

use tauri::State;

use crate::ctx::AppContext;
use crate::dto::wallpaper_window_dto::{
    CreateWallpaperWindowRequest, DestroyWallpaperWindowRequest,
};
use crate::dto::Validated;

use super::error::CommandResult;

/// 为指定显示器创建壁纸窗口
#[tauri::command]
pub async fn create_wallpaper_window(
    ctx: State<'_, AppContext>,
    req: Validated<CreateWallpaperWindowRequest>,
) -> CommandResult<()> {
    let req = req.into_inner();
    let app_ctx: &AppContext = ctx.inner();

    // 阶段 1：持锁建窗，拿到嵌入目标后立即释放锁
    let created = {
        let mut mgr = app_ctx.window_manager.lock().await;
        mgr.create_window(
            &req.monitor_id,
            req.x,
            req.y,
            req.width,
            req.height,
            req.extra_query.as_deref(),
        )?
    };

    let Some(created) = created else {
        return Ok(());
    };

    // 阶段 2：锁外把含 1 秒 Win32 调用的嵌入丢到阻塞线程池
    #[cfg(target_os = "windows")]
    if let Some(target) = created.embed_target {
        let result = tauri::async_runtime::spawn_blocking(move || {
            crate::platform::windows::desktop_embedder::embed_in_desktop(
                target.hwnd,
                target.x,
                target.y,
                target.width,
                target.height,
            )
        })
        .await;
        match result {
            Ok(Err(e)) => log::error!("桌面嵌入失败: {:#}", e),
            Err(e) => log::error!("桌面嵌入任务异常: {}", e),
            Ok(Ok(())) => {}
        }
    }

    // 阶段 3：重新持锁，嵌入完成后才显示窗口
    let mgr = app_ctx.window_manager.lock().await;
    mgr.show_window(&created.label);
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
    let mut mgr = app_ctx.window_manager.lock().await;
    mgr.destroy_window(&req.monitor_id);
    Ok(())
}

/// 销毁所有壁纸窗口
#[tauri::command]
pub async fn destroy_all_wallpaper_windows(ctx: State<'_, AppContext>) -> CommandResult<()> {
    let app_ctx: &AppContext = ctx.inner();
    let mut mgr = app_ctx.window_manager.lock().await;
    mgr.destroy_all();
    Ok(())
}

/// 隐藏所有壁纸窗口
#[tauri::command]
pub async fn hide_wallpaper_windows(ctx: State<'_, AppContext>) -> CommandResult<()> {
    let app_ctx: &AppContext = ctx.inner();
    let mgr = app_ctx.window_manager.lock().await;
    mgr.hide_all();
    Ok(())
}

/// 显示所有壁纸窗口
#[tauri::command]
pub async fn show_wallpaper_windows(ctx: State<'_, AppContext>) -> CommandResult<()> {
    let app_ctx: &AppContext = ctx.inner();
    let mgr = app_ctx.window_manager.lock().await;
    mgr.show_all();
    Ok(())
}

/// 获取当前已创建壁纸窗口的 monitor_id 列表
#[tauri::command]
pub async fn get_active_wallpaper_windows(
    ctx: State<'_, AppContext>,
) -> CommandResult<Vec<String>> {
    let app_ctx: &AppContext = ctx.inner();
    let mgr = app_ctx.window_manager.lock().await;
    Ok(mgr.get_active_window_ids())
}
