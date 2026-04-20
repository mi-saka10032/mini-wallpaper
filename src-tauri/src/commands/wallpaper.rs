use sea_orm::EntityTrait;
use tauri::{Manager, State};

use crate::entities::wallpaper;
use crate::services::wallpaper_service;
use crate::utils::ffmpeg;
use crate::AppState;

/// 获取壁纸列表
#[tauri::command]
pub async fn get_wallpapers(
    state: State<'_, AppState>,
) -> Result<Vec<wallpaper::Model>, String> {
    wallpaper::Entity::find()
        .all(&state.db)
        .await
        .map_err(|e| e.to_string())
}

/// 导入壁纸（接收文件路径数组，复制到应用目录，生成缩略图，写入数据库）
#[tauri::command]
pub async fn import_wallpapers(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<Vec<wallpaper::Model>, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;

    let wallpapers_dir = app_data_dir.join("wallpapers");
    let thumbnails_dir = app_data_dir.join("thumbnails");

    // 获取 ffmpeg 路径（优先 bundle 版，fallback 系统 PATH）
    let ffmpeg_path = ffmpeg::get_ffmpeg_path(&app);
    if !ffmpeg::is_ffmpeg_available(&ffmpeg_path) {
        eprintln!("[WARN] ffmpeg not found at '{}', video thumbnails will be skipped", ffmpeg_path);
    }

    wallpaper_service::import_batch(&state.db, paths, &wallpapers_dir, &thumbnails_dir, &ffmpeg_path)
        .await
        .map_err(|e| e.to_string())
}

/// 批量删除壁纸（删文件 + 删缩略图 + 删数据库记录）
#[tauri::command]
pub async fn delete_wallpapers(
    state: State<'_, AppState>,
    ids: Vec<i32>,
) -> Result<u64, String> {
    wallpaper_service::delete_batch(&state.db, ids)
        .await
        .map_err(|e| e.to_string())
}
