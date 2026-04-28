use tauri::{Manager, State};

use crate::ctx::AppContext;
use crate::dto::wallpaper_dto::{DeleteWallpapersRequest, ImportWallpapersRequest};
use crate::dto::Validated;
use crate::entities::wallpaper;
use crate::services::wallpaper_service;

/// 获取支持的壁纸文件扩展名列表
#[tauri::command]
pub fn get_supported_extensions() -> Vec<String> {
    wallpaper_service::get_supported_extensions()
}

/// 获取壁纸列表
#[tauri::command]
pub async fn get_wallpapers(
    ctx: State<'_, AppContext>,
) -> Result<Vec<wallpaper::Model>, String> {
    use sea_orm::EntityTrait;
    wallpaper::Entity::find()
        .all(&ctx.db)
        .await
        .map_err(|e| e.to_string())
}

/// 导入壁纸（接收文件路径数组，复制到应用目录，生成缩略图，写入数据库）
///
/// 图片/GIF 缩略图在此生成；视频缩略图由前端 canvas 抽帧后通过
/// `save_video_thumbnail` 单独写入。
#[tauri::command]
pub async fn import_wallpapers(
    ctx: State<'_, AppContext>,
    req: Validated<ImportWallpapersRequest>,
) -> Result<Vec<wallpaper::Model>, String> {
    let req = req.into_inner();
    let app_data_dir = ctx.app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;

    let wallpapers_dir = app_data_dir.join("wallpapers");
    let thumbnails_dir = app_data_dir.join("thumbnails");

    wallpaper_service::import_batch(&ctx.db, req.paths, &wallpapers_dir, &thumbnails_dir)
        .await
        .map_err(|e| e.to_string())
}

/// 保存视频缩略图（前端 canvas 抽帧后回传字节数据）
#[tauri::command]
pub async fn save_video_thumbnail(
    ctx: State<'_, AppContext>,
    wallpaper_id: i32,
    data: Vec<u8>,
) -> Result<String, String> {
    let app_data_dir = ctx.app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;

    let thumbnails_dir = app_data_dir.join("thumbnails");

    wallpaper_service::save_video_thumbnail(&ctx.db, wallpaper_id, data, &thumbnails_dir)
        .await
        .map_err(|e| e.to_string())
}

/// 批量删除壁纸（删文件 + 删缩略图 + 删数据库记录）
#[tauri::command]
pub async fn delete_wallpapers(
    ctx: State<'_, AppContext>,
    req: Validated<DeleteWallpapersRequest>,
) -> Result<u64, String> {
    let req = req.into_inner();
    wallpaper_service::delete_batch(&ctx.db, req.ids)
        .await
        .map_err(|e| e.to_string())
}
