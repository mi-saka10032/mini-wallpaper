use std::collections::HashSet;
use std::sync::Arc;

use tauri::{Manager, State};
use tokio::sync::Mutex;

use crate::ctx::AppContext;
use crate::dto::wallpaper_dto::{
    DeleteWallpapersRequest, ImportWallpapersRequest,
};
use crate::dto::Validated;
use crate::entities::wallpaper;
use crate::runtime::Scheduler;
use crate::services::{monitor_config_service, wallpaper_service};

use super::error::CommandResult;

/// 获取支持的壁纸文件扩展名列表
#[tauri::command]
pub fn get_supported_extensions() -> Vec<String> {
    wallpaper_service::get_supported_extensions()
}

/// 获取壁纸列表
#[tauri::command]
pub async fn get_wallpapers(
    ctx: State<'_, AppContext>,
) -> CommandResult<Vec<wallpaper::Model>> {
    Ok(wallpaper_service::get_all(&ctx.db).await?)
}

/// 根据 ID 获取单个壁纸详情
#[tauri::command]
pub async fn get_wallpaper(
    ctx: State<'_, AppContext>,
    id: i32,
) -> CommandResult<Option<wallpaper::Model>> {
    Ok(wallpaper_service::get_by_id(&ctx.db, id).await?)
}

/// 导入壁纸（接收文件路径数组，复制到应用目录，生成缩略图，写入数据库）
///
/// 图片/GIF 缩略图在此生成；视频缩略图由前端 canvas 抽帧后通过
/// `save_video_thumbnail` 单独写入。
#[tauri::command]
pub async fn import_wallpapers(
    ctx: State<'_, AppContext>,
    req: Validated<ImportWallpapersRequest>,
) -> CommandResult<Vec<wallpaper::Model>> {
    let req = req.into_inner();
    let app_data_dir = ctx.app_handle.path().app_data_dir()?;

    let wallpapers_dir = app_data_dir.join("wallpapers");
    let thumbnails_dir = app_data_dir.join("thumbnails");

    Ok(wallpaper_service::import_batch(&ctx.db, req.paths, &wallpapers_dir, &thumbnails_dir).await?)
}

/// 通过字节方式导入单个壁纸（H5 拖拽场景，raw body 直传）
///
/// 与 `import_wallpapers` 区别：直接接收字节内容，不依赖 Tauri 注入的
/// File.path 属性，适用于 dragDropEnabled = false 场景。
///
/// 字节数据通过 Tauri v2 raw body（`InvokeBody::Raw`）直传，避免 JSON
/// 数组序列化开销；文件名经 `fileName` 请求头传入（前端 encodeURIComponent 编码）。
#[tauri::command]
pub async fn import_wallpaper_bytes(
    ctx: State<'_, AppContext>,
    request: tauri::ipc::Request<'_>,
) -> CommandResult<wallpaper::Model> {
    let tauri::ipc::InvokeBody::Raw(data) = request.body() else {
        return Err("import_wallpaper_bytes 需要 raw body（Uint8Array）".into());
    };

    let file_name = request
        .headers()
        .get("fileName")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| urlencoding::decode(s).ok().map(|c| c.into_owned()))
        .ok_or("import_wallpaper_bytes 缺少有效的 fileName 请求头")?;

    let app_data_dir = ctx.app_handle.path().app_data_dir()?;
    let wallpapers_dir = app_data_dir.join("wallpapers");
    let thumbnails_dir = app_data_dir.join("thumbnails");

    Ok(wallpaper_service::import_single_from_bytes(
        &ctx.db,
        file_name,
        data.clone(),
        &wallpapers_dir,
        &thumbnails_dir,
    )
    .await?)
}

/// 保存视频缩略图（前端 canvas 抽帧后回传字节数据）
///
/// 字节数据通过 Tauri v2 raw body（`InvokeBody::Raw`）直传，避免 JSON
/// 序列化开销；`wallpaperId` 经请求头传入。
#[tauri::command]
pub async fn save_video_thumbnail(
    ctx: State<'_, AppContext>,
    request: tauri::ipc::Request<'_>,
) -> CommandResult<String> {
    let tauri::ipc::InvokeBody::Raw(data) = request.body() else {
        return Err("save_video_thumbnail 需要 raw body（Uint8Array）".into());
    };

    let wallpaper_id: i32 = request
        .headers()
        .get("wallpaperId")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .ok_or("save_video_thumbnail 缺少有效的 wallpaperId 请求头")?;

    let app_data_dir = ctx.app_handle.path().app_data_dir()?;
    let thumbnails_dir = app_data_dir.join("thumbnails");

    Ok(
        wallpaper_service::save_video_thumbnail(&ctx.db, wallpaper_id, data.clone(), &thumbnails_dir)
            .await?,
    )
}

/// 批量删除壁纸 —— 语义为「移入回收站」（软删除 + 窗口/定时器联动）
///
/// 不删除磁盘文件、不解除收藏夹与标签关联，以便恢复。但仍会解除
/// `monitor_configs.wallpaper_id` 引用并触发联动：正在显示的壁纸进了回收站，
/// 桌面必须立刻切走或清空，这一用户可感知行为与硬删除时保持一致。
///
/// 删除后的联动逻辑委托给 `Scheduler::on_wallpapers_deleted`，
/// Command 层只负责数据操作 + 一行调度器调用。
#[tauri::command]
pub async fn delete_wallpapers(
    ctx: State<'_, AppContext>,
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
    req: Validated<DeleteWallpapersRequest>,
) -> CommandResult<u64> {
    let req = req.into_inner();
    let deleted_ids: HashSet<i32> = req.ids.iter().copied().collect();

    // 1. 预先查出引用这些壁纸的完整 config（内存快照，删除后 wallpaper_id 会被置空）
    let affected_configs = monitor_config_service::get_configs_by_wallpaper_ids(&ctx.db, &req.ids).await?;

    // 2. 执行数据层软删除（打 deleted_at 标记 + sort_order 哨兵与连续化重排 + monitor_config.wallpaper_id 置空）
    let deleted = wallpaper_service::trash_batch(&ctx.db, req.ids).await?;

    // 3. 联动处理（定时器管理 + 壁纸窗口通知）— 一行搞定
    let mut sched = scheduler.lock().await;
    sched.on_wallpapers_deleted(&affected_configs, &deleted_ids).await;

    Ok(deleted)
}

/// 获取回收站内的壁纸列表（按移入时间倒序）
#[tauri::command]
pub async fn get_trashed_wallpapers(
    ctx: State<'_, AppContext>,
) -> CommandResult<Vec<wallpaper::Model>> {
    Ok(wallpaper_service::get_trashed(&ctx.db).await?)
}

/// 从回收站恢复壁纸（批量）
///
/// 恢复后壁纸重新出现在主网格，并按 `max(sort_order) + 1` 追加回原收藏夹末尾。
/// 无需调度器联动：恢复不会改变任何显示器当前的绑定状态。
#[tauri::command]
pub async fn restore_wallpapers(
    ctx: State<'_, AppContext>,
    req: Validated<DeleteWallpapersRequest>,
) -> CommandResult<u64> {
    let req = req.into_inner();
    Ok(wallpaper_service::restore_batch(&ctx.db, req.ids).await?)
}

/// 彻底删除壁纸（不可恢复：删数据库记录 + 关联 + 磁盘文件）
///
/// 目标壁纸已在回收站中，其 `monitor_configs` 引用在移入时已被解除，
/// 故此处仍走一次联动以覆盖「直接对未入回收站记录调用」的边界情形。
#[tauri::command]
pub async fn purge_wallpapers(
    ctx: State<'_, AppContext>,
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
    req: Validated<DeleteWallpapersRequest>,
) -> CommandResult<u64> {
    let req = req.into_inner();
    let deleted_ids: HashSet<i32> = req.ids.iter().copied().collect();

    let affected_configs =
        monitor_config_service::get_configs_by_wallpaper_ids(&ctx.db, &req.ids).await?;

    let deleted = wallpaper_service::delete_batch(&ctx.db, req.ids).await?;

    let mut sched = scheduler.lock().await;
    sched.on_wallpapers_deleted(&affected_configs, &deleted_ids).await;

    Ok(deleted)
}

/// 清空回收站（彻底删除其中全部壁纸，不可恢复）
#[tauri::command]
pub async fn empty_trash(
    ctx: State<'_, AppContext>,
) -> CommandResult<u64> {
    Ok(wallpaper_service::empty_trash(&ctx.db).await?)
}