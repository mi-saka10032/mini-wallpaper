use sea_orm::DatabaseConnection;
use tauri::State;

use crate::entities::{collection, wallpaper};
use crate::services::collection_service;

/// 获取所有收藏夹
#[tauri::command]
pub async fn get_collections(
    db: State<'_, DatabaseConnection>,
) -> Result<Vec<collection::Model>, String> {
    collection_service::get_all(db.inner())
        .await
        .map_err(|e| e.to_string())
}

/// 创建收藏夹
#[tauri::command]
pub async fn create_collection(
    db: State<'_, DatabaseConnection>,
    name: String,
) -> Result<collection::Model, String> {
    collection_service::create(db.inner(), name)
        .await
        .map_err(|e| e.to_string())
}

/// 重命名收藏夹
#[tauri::command]
pub async fn rename_collection(
    db: State<'_, DatabaseConnection>,
    id: i32,
    name: String,
) -> Result<collection::Model, String> {
    collection_service::rename(db.inner(), id, name)
        .await
        .map_err(|e| e.to_string())
}

/// 删除收藏夹
#[tauri::command]
pub async fn delete_collection(
    db: State<'_, DatabaseConnection>,
    id: i32,
) -> Result<(), String> {
    collection_service::delete(db.inner(), id)
        .await
        .map_err(|e| e.to_string())
}

/// 获取收藏夹内的壁纸列表
#[tauri::command]
pub async fn get_collection_wallpapers(
    db: State<'_, DatabaseConnection>,
    collection_id: i32,
) -> Result<Vec<wallpaper::Model>, String> {
    collection_service::get_wallpapers(db.inner(), collection_id)
        .await
        .map_err(|e| e.to_string())
}

/// 向收藏夹添加壁纸
#[tauri::command]
pub async fn add_wallpapers_to_collection(
    db: State<'_, DatabaseConnection>,
    collection_id: i32,
    wallpaper_ids: Vec<i32>,
) -> Result<u32, String> {
    collection_service::add_wallpapers(db.inner(), collection_id, wallpaper_ids)
        .await
        .map_err(|e| e.to_string())
}

/// 从收藏夹移除壁纸
#[tauri::command]
pub async fn remove_wallpapers_from_collection(
    db: State<'_, DatabaseConnection>,
    collection_id: i32,
    wallpaper_ids: Vec<i32>,
) -> Result<u64, String> {
    collection_service::remove_wallpapers(db.inner(), collection_id, wallpaper_ids)
        .await
        .map_err(|e| e.to_string())
}

/// 重新排序收藏夹内的壁纸
#[tauri::command]
pub async fn reorder_collection_wallpapers(
    db: State<'_, DatabaseConnection>,
    collection_id: i32,
    wallpaper_ids: Vec<i32>,
) -> Result<(), String> {
    collection_service::reorder_wallpapers(db.inner(), collection_id, wallpaper_ids)
        .await
        .map_err(|e| e.to_string())
}
