use std::collections::HashSet;
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

/// 删除收藏夹（数据层删除 + 窗口/定时器联动）
///
/// 删除后的联动逻辑委托给 `Scheduler::on_collection_deleted`，
/// Command 层只负责数据操作 + 一行调度器调用。
#[tauri::command]
pub async fn delete_collection(
    ctx: State<'_, AppContext>,
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
    req: Validated<DeleteCollectionRequest>,
) -> CommandResult<()> {
    let req = req.into_inner();

    // 1. 预先查出引用该收藏夹的完整 config（内存快照，删除后 collection_id 会被置空）
    let affected_configs = monitor_config_service::get_configs_by_collection_id(&ctx.db, req.id).await?;

    // 2. 执行数据层删除（清理关联记录 + 置空 monitor_config.collection_id + 删除收藏夹）
    collection_service::delete(&ctx.db, req.id).await?;

    // 3. 联动处理（定时器管理 + 壁纸窗口通知）— 一行搞定
    let mut sched = scheduler.lock().await;
    sched.on_collection_deleted(&affected_configs).await;

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

/// 从收藏夹移除壁纸（数据层删除 + 窗口/定时器联动）
///
/// 移除后的联动逻辑委托给 `Scheduler::on_wallpapers_removed_from_collection`，
/// Command 层只负责数据操作 + 一行调度器调用。
#[tauri::command]
pub async fn remove_wallpapers_from_collection(
    ctx: State<'_, AppContext>,
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
    req: Validated<RemoveWallpapersRequest>,
) -> CommandResult<u64> {
    let req = req.into_inner();
    let collection_id = req.collection_id;
    let removing_ids: HashSet<i32> = req.wallpaper_ids.iter().copied().collect();

    // 1. 查出所有绑定该收藏夹的 active config（用于联动判断）
    let bound_configs = monitor_config_service::get_configs_by_collection_id(&ctx.db, collection_id).await?;

    // 2. 执行数据层移除
    let removed = collection_service::remove_wallpapers(&ctx.db, collection_id, req.wallpaper_ids).await?;

    if removed == 0 {
        return Ok(removed);
    }

    // 3. 联动处理（定时器管理 + 壁纸窗口通知）— 一行搞定
    let mut sched = scheduler.lock().await;
    sched.on_wallpapers_removed_from_collection(&bound_configs, collection_id, &removing_ids).await;

    Ok(removed)
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
