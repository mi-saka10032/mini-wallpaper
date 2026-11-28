//! 目录监听命令层。
//!
//! 所有写命令只调 `watched_folder_service`，不复用 `trash_batch` /
//! `delete_batch`（后者会 remove_file 源文件、走软删除语义）。

use std::sync::atomic::Ordering;
use std::sync::Arc;

use tauri::State;

use crate::context::AppContext;
use crate::dto::watched_folder_dto::{
    AddWatchedFolderRequest, DeleteWatchedWallpapersRequest, RemoveWatchedFolderRequest,
    SetActiveViewRequest, SyncWatchedFolderRequest,
};
use crate::dto::Validated;
use crate::services::watched_folder_service;

use super::error::CommandResult;

/// 监听目录列表（含失效条目，弹窗里提示用户）
#[tauri::command]
pub async fn list_watched_folders(
    ctx: State<'_, Arc<AppContext>>,
) -> CommandResult<Vec<watched_folder_service::WatchedFolderView>> {
    Ok(watched_folder_service::list(&ctx.db).await?)
}

/// opener 选中目录后调用：新建或重选失效路径后重新激活。
/// 返回新收藏夹 id 与目录名，前端据此弹「手动更名」提示。
#[tauri::command]
pub async fn add_watched_folder(
    ctx: State<'_, Arc<AppContext>>,
    req: Validated<AddWatchedFolderRequest>,
) -> CommandResult<watched_folder_service::AddWatchedFolderResult> {
    let app_ctx: &AppContext = ctx.inner();
    Ok(watched_folder_service::create_or_reactivate(app_ctx, &req.path).await?)
}

/// 手动移除监听（不碰源目录里的文件）
#[tauri::command]
pub async fn remove_watched_folder(
    ctx: State<'_, Arc<AppContext>>,
    req: Validated<RemoveWatchedFolderRequest>,
) -> CommandResult<()> {
    let app_ctx: &AppContext = ctx.inner();
    Ok(watched_folder_service::remove_folder(app_ctx, req.id).await?)
}

/// 进入监听夹 / 手动刷新：强制同步，返回本次新增的视频 id 供前端补抽帧。
/// 路径失效时上抛 `WATCHED_PATH_INVALID`，前端 toast + activeId=0 + 刷新 collections。
#[tauri::command]
pub async fn sync_watched_folder(
    ctx: State<'_, Arc<AppContext>>,
    req: Validated<SyncWatchedFolderRequest>,
) -> CommandResult<watched_folder_service::SyncReport> {
    let app_ctx: &AppContext = ctx.inner();
    Ok(watched_folder_service::sync_by_collection(app_ctx, req.collection_id).await?)
}

/// 主窗口切换视图时同步 active 夹 id；后台轮询据此暂停该夹的完整 diff。
#[tauri::command]
pub fn set_active_view(ctx: State<'_, Arc<AppContext>>, req: Validated<SetActiveViewRequest>) {
    ctx.active_collection_id
        .store(req.active_id, Ordering::Relaxed);
}

/// 监听壁纸删除：源文件移出 + 硬删 DB，逐条处理，返回实际成功数。
#[tauri::command]
pub async fn delete_watched_wallpapers(
    ctx: State<'_, Arc<AppContext>>,
    req: Validated<DeleteWatchedWallpapersRequest>,
) -> CommandResult<usize> {
    let app_ctx: &AppContext = ctx.inner();
    Ok(watched_folder_service::delete_watched_wallpapers(app_ctx, req.ids.clone()).await?)
}
