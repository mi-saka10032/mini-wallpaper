use tauri::State;

use crate::entities::{collection, wallpaper};
use crate::services::collection_service;
use crate::AppState;

/// 获取所有收藏夹
#[tauri::command]
pub async fn get_collections(
    state: State<'_, AppState>,
) -> Result<Vec<collection::Model>, String> {
    collection_service::get_all(&state.db)
        .await
        .map_err(|e| e.to_string())
}

/// 创建收藏夹
#[tauri::command]
pub async fn create_collection(
    state: State<'_, AppState>,
    name: String,
) -> Result<collection::Model, String> {
    collection_service::create(&state.db, name)
        .await
        .map_err(|e| e.to_string())
}

/// 重命名收藏夹
#[tauri::command]
pub async fn rename_collection(
    state: State<'_, AppState>,
    id: i32,
    name: String,
) -> Result<collection::Model, String> {
    collection_service::rename(&state.db, id, name)
        .await
        .map_err(|e| e.to_string())
}

/// 删除收藏夹
#[tauri::command]
pub async fn delete_collection(
    state: State<'_, AppState>,
    id: i32,
) -> Result<(), String> {
    collection_service::delete(&state.db, id)
        .await
        .map_err(|e| e.to_string())
}

/// 获取收藏夹内的壁纸列表
#[tauri::command]
pub async fn get_collection_wallpapers(
    state: State<'_, AppState>,
    collection_id: i32,
) -> Result<Vec<wallpaper::Model>, String> {
    collection_service::get_wallpapers(&state.db, collection_id)
        .await
        .map_err(|e| e.to_string())
}

/// 向收藏夹添加壁纸
#[tauri::command]
pub async fn add_wallpapers_to_collection(
    state: State<'_, AppState>,
    collection_id: i32,
    wallpaper_ids: Vec<i32>,
) -> Result<u32, String> {
    collection_service::add_wallpapers(&state.db, collection_id, wallpaper_ids)
        .await
        .map_err(|e| e.to_string())
}

/// 从收藏夹移除壁纸
#[tauri::command]
pub async fn remove_wallpapers_from_collection(
    state: State<'_, AppState>,
    collection_id: i32,
    wallpaper_ids: Vec<i32>,
) -> Result<u64, String> {
    collection_service::remove_wallpapers(&state.db, collection_id, wallpaper_ids)
        .await
        .map_err(|e| e.to_string())
}

/// 重新排序收藏夹内的壁纸
#[tauri::command]
pub async fn reorder_collection_wallpapers(
    state: State<'_, AppState>,
    collection_id: i32,
    wallpaper_ids: Vec<i32>,
) -> Result<(), String> {
    collection_service::reorder_wallpapers(&state.db, collection_id, wallpaper_ids)
        .await
        .map_err(|e| e.to_string())
}
