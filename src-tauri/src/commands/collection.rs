use tauri::State;

use crate::ctx::AppContext;
use crate::dto::collection_dto::{
    AddWallpapersRequest, CreateCollectionRequest, DeleteCollectionRequest,
    GetCollectionWallpapersRequest, RemoveWallpapersRequest, RenameCollectionRequest,
    ReorderWallpapersRequest,
};
use crate::dto::Validated;
use crate::entities::{collection, wallpaper};
use crate::services::collection_service;

/// 获取所有收藏夹
#[tauri::command]
pub async fn get_collections(
    ctx: State<'_, AppContext>,
) -> Result<Vec<collection::Model>, String> {
    collection_service::get_all(&ctx.db)
        .await
        .map_err(|e| e.to_string())
}

/// 创建收藏夹
#[tauri::command]
pub async fn create_collection(
    ctx: State<'_, AppContext>,
    req: Validated<CreateCollectionRequest>,
) -> Result<collection::Model, String> {
    let req = req.into_inner();
    collection_service::create(&ctx.db, req.name)
        .await
        .map_err(|e| e.to_string())
}

/// 重命名收藏夹
#[tauri::command]
pub async fn rename_collection(
    ctx: State<'_, AppContext>,
    req: Validated<RenameCollectionRequest>,
) -> Result<collection::Model, String> {
    let req = req.into_inner();
    collection_service::rename(&ctx.db, req.id, req.name)
        .await
        .map_err(|e| e.to_string())
}

/// 删除收藏夹
#[tauri::command]
pub async fn delete_collection(
    ctx: State<'_, AppContext>,
    req: Validated<DeleteCollectionRequest>,
) -> Result<(), String> {
    let req = req.into_inner();
    collection_service::delete(&ctx.db, req.id)
        .await
        .map_err(|e| e.to_string())
}

/// 获取收藏夹内的壁纸列表
#[tauri::command]
pub async fn get_collection_wallpapers(
    ctx: State<'_, AppContext>,
    req: Validated<GetCollectionWallpapersRequest>,
) -> Result<Vec<wallpaper::Model>, String> {
    let req = req.into_inner();
    collection_service::get_wallpapers(&ctx.db, req.collection_id)
        .await
        .map_err(|e| e.to_string())
}

/// 向收藏夹添加壁纸
#[tauri::command]
pub async fn add_wallpapers_to_collection(
    ctx: State<'_, AppContext>,
    req: Validated<AddWallpapersRequest>,
) -> Result<u32, String> {
    let req = req.into_inner();
    collection_service::add_wallpapers(&ctx.db, req.collection_id, req.wallpaper_ids)
        .await
        .map_err(|e| e.to_string())
}

/// 从收藏夹移除壁纸
#[tauri::command]
pub async fn remove_wallpapers_from_collection(
    ctx: State<'_, AppContext>,
    req: Validated<RemoveWallpapersRequest>,
) -> Result<u64, String> {
    let req = req.into_inner();
    collection_service::remove_wallpapers(&ctx.db, req.collection_id, req.wallpaper_ids)
        .await
        .map_err(|e| e.to_string())
}

/// 重新排序收藏夹内的壁纸
#[tauri::command]
pub async fn reorder_collection_wallpapers(
    ctx: State<'_, AppContext>,
    req: Validated<ReorderWallpapersRequest>,
) -> Result<(), String> {
    let req = req.into_inner();
    collection_service::reorder_wallpapers(&ctx.db, req.collection_id, req.wallpaper_ids)
        .await
        .map_err(|e| e.to_string())
}
