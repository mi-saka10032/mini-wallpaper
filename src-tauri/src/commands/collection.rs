use std::sync::Arc;

use tauri::State;
use tokio::sync::Mutex;

use crate::application::{self, AppServices};
use crate::context::AppContext;
use crate::dto::collection_dto::{
    AddWallpapersRequest, CreateCollectionRequest, CreateSmartCollectionRequest,
    DeleteCollectionRequest, GetCollectionWallpapersRequest, PreviewSmartCountRequest,
    RemoveWallpapersRequest, RenameCollectionRequest, ReorderCollectionsRequest,
    ReorderWallpapersRequest, ToggleFavoriteRequest, UpdateSmartCollectionRequest,
};
use crate::dto::Validated;
use crate::entities::{collection, wallpaper};
use crate::services::collection_service;
use crate::tasks::registry::TaskRegistry;

use super::error::CommandResult;

/// 获取所有收藏夹
#[tauri::command]
pub async fn get_collections(
    ctx: State<'_, Arc<AppContext>>,
) -> CommandResult<Vec<collection::Model>> {
    let app_ctx: &AppContext = ctx.inner();
    Ok(collection_service::get_all(&app_ctx.db).await?)
}

/// 创建收藏夹
#[tauri::command]
pub async fn create_collection(
    ctx: State<'_, Arc<AppContext>>,
    req: Validated<CreateCollectionRequest>,
) -> CommandResult<collection::Model> {
    let req = req.into_inner();
    let app_ctx: &AppContext = ctx.inner();
    Ok(collection_service::create(&app_ctx.db, req.name).await?)
}

/// 重命名收藏夹
#[tauri::command]
pub async fn rename_collection(
    ctx: State<'_, Arc<AppContext>>,
    req: Validated<RenameCollectionRequest>,
) -> CommandResult<collection::Model> {
    let req = req.into_inner();
    let app_ctx: &AppContext = ctx.inner();
    Ok(collection_service::rename(&app_ctx.db, req.id, req.name).await?)
}

/// 删除收藏夹（数据层删除 + 窗口/定时器联动）
///
/// 删除后的联动逻辑委托给 `TaskRegistry::on_collection_deleted`，
/// Command 层只负责数据操作 + 一行调度器调用。
#[tauri::command]
pub async fn delete_collection(
    ctx: State<'_, Arc<AppContext>>,
    scheduler: State<'_, Arc<Mutex<TaskRegistry>>>,
    req: Validated<DeleteCollectionRequest>,
) -> CommandResult<()> {
    let svc = AppServices::new(ctx.inner(), scheduler.inner());
    Ok(application::collection::delete_collection(&svc, req.into_inner().id).await?)
}

/// 获取收藏夹内的壁纸列表
#[tauri::command]
pub async fn get_collection_wallpapers(
    ctx: State<'_, Arc<AppContext>>,
    req: Validated<GetCollectionWallpapersRequest>,
) -> CommandResult<Vec<wallpaper::Model>> {
    let req = req.into_inner();
    let app_ctx: &AppContext = ctx.inner();
    Ok(collection_service::get_wallpapers(&app_ctx.db, req.collection_id).await?)
}

/// 向收藏夹添加壁纸
#[tauri::command]
pub async fn add_wallpapers_to_collection(
    ctx: State<'_, Arc<AppContext>>,
    req: Validated<AddWallpapersRequest>,
) -> CommandResult<u32> {
    let req = req.into_inner();
    let app_ctx: &AppContext = ctx.inner();
    Ok(
        collection_service::add_wallpapers(&app_ctx.db, req.collection_id, req.wallpaper_ids)
            .await?,
    )
}

/// 从收藏夹移除壁纸（数据层删除 + 窗口/定时器联动）
///
/// 移除后的联动逻辑委托给 `TaskRegistry::on_wallpapers_removed_from_collection`，
/// Command 层只负责数据操作 + 一行调度器调用。
#[tauri::command]
pub async fn remove_wallpapers_from_collection(
    ctx: State<'_, Arc<AppContext>>,
    scheduler: State<'_, Arc<Mutex<TaskRegistry>>>,
    req: Validated<RemoveWallpapersRequest>,
) -> CommandResult<u64> {
    let req = req.into_inner();
    let svc = AppServices::new(ctx.inner(), scheduler.inner());
    Ok(application::collection::remove_wallpapers_from_collection(
        &svc,
        req.collection_id,
        req.wallpaper_ids,
    )
    .await?)
}

/// 重新排序收藏夹列表（内置收藏夹固定顶部，不参与）
#[tauri::command]
pub async fn reorder_collections(
    ctx: State<'_, Arc<AppContext>>,
    req: Validated<ReorderCollectionsRequest>,
) -> CommandResult<()> {
    let req = req.into_inner();
    let app_ctx: &AppContext = ctx.inner();
    Ok(collection_service::reorder_collections(&app_ctx.db, req.collection_ids).await?)
}

/// 重新排序收藏夹内的壁纸
#[tauri::command]
pub async fn reorder_collection_wallpapers(
    ctx: State<'_, Arc<AppContext>>,
    req: Validated<ReorderWallpapersRequest>,
) -> CommandResult<()> {
    let req = req.into_inner();
    let app_ctx: &AppContext = ctx.inner();
    Ok(
        collection_service::reorder_wallpapers(&app_ctx.db, req.collection_id, req.wallpaper_ids)
            .await?,
    )
}

/// 切换壁纸在内置「我喜欢」收藏夹中的收藏状态
///
/// 供前端红心按钮 / 右键菜单调用（作用于卡片壁纸）。与全局快捷键 / 托盘的
/// ToggleFavorite（作用于当前显示壁纸）共享 `TaskRegistry::toggle_favorite`
/// 单一入口：动态定位内置收藏夹、幂等 add/remove、移除联动、事件广播。
///
/// 返回切换后的收藏状态：`true` = 已收藏，`false` = 已取消收藏。
#[tauri::command]
pub async fn toggle_favorite(
    ctx: State<'_, Arc<AppContext>>,
    scheduler: State<'_, Arc<Mutex<TaskRegistry>>>,
    req: Validated<ToggleFavoriteRequest>,
) -> CommandResult<bool> {
    let req = req.into_inner();
    let svc = AppServices::new(ctx.inner(), scheduler.inner());
    Ok(application::collection::toggle_favorite(&svc, req.wallpaper_id).await?)
}

/// 创建智能收藏夹（规则经白名单校验后落库，成员不物化）
#[tauri::command]
pub async fn create_smart_collection(
    ctx: State<'_, Arc<AppContext>>,
    req: Validated<CreateSmartCollectionRequest>,
) -> CommandResult<collection::Model> {
    let req = req.into_inner();
    let app_ctx: &AppContext = ctx.inner();
    Ok(collection_service::create_smart(&app_ctx.db, req.name, req.rule_json).await?)
}

/// 更新智能收藏夹规则（可同时改名）
///
/// 规则变更即命中集变更；绑定该收藏夹的显示器会在下一次轮播 tick 自动生效。
#[tauri::command]
pub async fn update_smart_collection(
    ctx: State<'_, Arc<AppContext>>,
    req: Validated<UpdateSmartCollectionRequest>,
) -> CommandResult<collection::Model> {
    let req = req.into_inner();
    let app_ctx: &AppContext = ctx.inner();
    Ok(collection_service::update_smart(&app_ctx.db, req.id, req.name, req.rule_json).await?)
}

/// 预览规则命中数（未落库，供创建 / 编辑时实时提示「当前匹配 N 张」）
#[tauri::command]
pub async fn preview_smart_count(
    ctx: State<'_, Arc<AppContext>>,
    req: Validated<PreviewSmartCountRequest>,
) -> CommandResult<u64> {
    let req = req.into_inner();
    let app_ctx: &AppContext = ctx.inner();
    Ok(collection_service::preview_smart_count(&app_ctx.db, req.rule_json).await?)
}
