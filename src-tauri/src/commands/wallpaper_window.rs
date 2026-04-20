//! 壁纸窗口管理相关命令

use tauri::State;

use crate::services::wallpaper_window_service::WallpaperWindowManagerState;

/// 为指定显示器创建壁纸窗口
#[tauri::command]
pub async fn create_wallpaper_window(
    app: tauri::AppHandle,
    manager: State<'_, WallpaperWindowManagerState>,
    monitor_id: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    extra_query: Option<String>,
) -> Result<(), String> {
    let mut mgr = manager.lock().await;
    mgr.create_window(&app, &monitor_id, x, y, width, height, extra_query.as_deref())
}

/// 销毁指定显示器的壁纸窗口
#[tauri::command]
pub async fn destroy_wallpaper_window(
    app: tauri::AppHandle,
    manager: State<'_, WallpaperWindowManagerState>,
    monitor_id: String,
) -> Result<(), String> {
    let mut mgr = manager.lock().await;
    mgr.destroy_window(&app, &monitor_id);
    Ok(())
}

/// 销毁所有壁纸窗口
#[tauri::command]
pub async fn destroy_all_wallpaper_windows(
    app: tauri::AppHandle,
    manager: State<'_, WallpaperWindowManagerState>,
) -> Result<(), String> {
    let mut mgr = manager.lock().await;
    mgr.destroy_all(&app);
    Ok(())
}

/// 隐藏所有壁纸窗口
#[tauri::command]
pub async fn hide_wallpaper_windows(
    app: tauri::AppHandle,
    manager: State<'_, WallpaperWindowManagerState>,
) -> Result<(), String> {
    let mgr = manager.lock().await;
    mgr.hide_all(&app);
    Ok(())
}

/// 显示所有壁纸窗口
#[tauri::command]
pub async fn show_wallpaper_windows(
    app: tauri::AppHandle,
    manager: State<'_, WallpaperWindowManagerState>,
) -> Result<(), String> {
    let mgr = manager.lock().await;
    mgr.show_all(&app);
    Ok(())
}
