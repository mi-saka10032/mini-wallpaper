use std::sync::Arc;

use tauri::State;
use tokio::sync::Mutex;

use crate::ctx::AppContext;
use crate::dto::collection_dto::{
    AddWallpapersRequest, CreateCollectionRequest, DeleteCollectionRequest,
    GetCollectionWallpapersRequest, RemoveWallpapersRequest, RenameCollectionRequest,
    ReorderWallpapersRequest,
};
use crate::dto::Validated;
use crate::entities::{collection, wallpaper};
use crate::runtime::carousel::carousel_key;
use crate::runtime::Scheduler;
use crate::services::{collection_service, monitor_config_service};

use super::error::CommandResult;

/// 获取所有收藏夹
#[tauri::command]
pub async fn get_collections(
    ctx: State<'_, AppContext>,
) -> CommandResult<Vec<collection::Model>> {
    Ok(collection_service::get_all(&ctx.db).await?)
}

/// 创建收藏夹
#[tauri::command]
pub async fn create_collection(
    ctx: State<'_, AppContext>,
    req: Validated<CreateCollectionRequest>,
) -> CommandResult<collection::Model> {
    let req = req.into_inner();
    Ok(collection_service::create(&ctx.db, req.name).await?)
}

/// 重命名收藏夹
#[tauri::command]
pub async fn rename_collection(
    ctx: State<'_, AppContext>,
    req: Validated<RenameCollectionRequest>,
) -> CommandResult<collection::Model> {
    let req = req.into_inner();
    Ok(collection_service::rename(&ctx.db, req.id, req.name).await?)
}

/// 删除收藏夹
///
/// 删除成功后停止所有引用该收藏夹的轮播定时器，保持 service 层纯数据操作
#[tauri::command]
pub async fn delete_collection(
    ctx: State<'_, AppContext>,
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
    req: Validated<DeleteCollectionRequest>,
) -> CommandResult<()> {
    let req = req.into_inner();

    // 1. 预先查出引用该收藏夹的 monitor_id 列表（内存快照，后续删除不影响）
    let monitor_ids = monitor_config_service::get_monitor_ids_by_collection_id(&ctx.db, req.id).await?;

    // 2. 执行数据层删除（清理关联记录 + 置空 monitor_config.collection_id + 删除收藏夹）
    collection_service::delete(&ctx.db, req.id).await?;

    // 3. 删除成功后，停止引用该收藏夹的轮播定时器
    {
        let mut sched = scheduler.lock().await;
        for mid in &monitor_ids {
            sched.stop(&carousel_key(mid));
        }
    }

    Ok(())
}

/// 获取收藏夹内的壁纸列表
#[tauri::command]
pub async fn get_collection_wallpapers(
    ctx: State<'_, AppContext>,
    req: Validated<GetCollectionWallpapersRequest>,
) -> CommandResult<Vec<wallpaper::Model>> {
    let req = req.into_inner();
    Ok(collection_service::get_wallpapers(&ctx.db, req.collection_id).await?)
}

/// 向收藏夹添加壁纸
#[tauri::command]
pub async fn add_wallpapers_to_collection(
    ctx: State<'_, AppContext>,
    req: Validated<AddWallpapersRequest>,
) -> CommandResult<u32> {
    let req = req.into_inner();
    Ok(collection_service::add_wallpapers(&ctx.db, req.collection_id, req.wallpaper_ids).await?)
}

/// 从收藏夹移除壁纸
#[tauri::command]
pub async fn remove_wallpapers_from_collection(
    ctx: State<'_, AppContext>,
    req: Validated<RemoveWallpapersRequest>,
) -> CommandResult<u64> {
    let req = req.into_inner();
    Ok(collection_service::remove_wallpapers(&ctx.db, req.collection_id, req.wallpaper_ids).await?)
}

/// 重新排序收藏夹内的壁纸
#[tauri::command]
pub async fn reorder_collection_wallpapers(
    ctx: State<'_, AppContext>,
    req: Validated<ReorderWallpapersRequest>,
) -> CommandResult<()> {
    let req = req.into_inner();
    Ok(collection_service::reorder_wallpapers(&ctx.db, req.collection_id, req.wallpaper_ids).await?)
}
