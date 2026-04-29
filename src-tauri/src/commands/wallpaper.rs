use std::collections::HashSet;
use std::sync::Arc;

use tauri::{Manager, State};
use tokio::sync::Mutex;

use crate::ctx::AppContext;
use crate::dto::wallpaper_dto::{DeleteWallpapersRequest, ImportWallpapersRequest};
use crate::dto::Validated;
use crate::entities::wallpaper;
use crate::runtime::carousel::carousel_key;
use crate::runtime::Scheduler;
use crate::services::{monitor_config_service, wallpaper_service};

use super::error::CommandResult;
use super::playback_cascade;

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

/// 保存视频缩略图（前端 canvas 抽帧后回传字节数据）
#[tauri::command]
pub async fn save_video_thumbnail(
    ctx: State<'_, AppContext>,
    wallpaper_id: i32,
    data: Vec<u8>,
) -> CommandResult<String> {
    let app_data_dir = ctx.app_handle.path().app_data_dir()?;
    let thumbnails_dir = app_data_dir.join("thumbnails");

    Ok(wallpaper_service::save_video_thumbnail(&ctx.db, wallpaper_id, data, &thumbnails_dir).await?)
}

/// 批量删除壁纸（删文件 + 删缩略图 + 删数据库记录 + 窗口/定时器联动）
///
/// 删除后根据每个受影响显示器的状态执行联动：
/// - 场景 1b：单张模式，壁纸被删 → 清空壁纸窗口
/// - 场景 1c：收藏夹模式，当前壁纸被删 → 切换到下一张 + 重启定时器
/// - 场景 1c 边界：收藏夹删后只剩 0 张 → 退化为清空
/// - 场景 1d：收藏夹模式，删除的不是当前壁纸 → 检查剩余数量，≤1 则停止定时器
/// - 场景 1e：display_mode 同步模式下广播到所有窗口
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

    // 2. 执行数据层删除（删文件 + 缩略图 + collection_wallpapers 关联 + monitor_config.wallpaper_id 置空 + 数据库记录）
    let deleted = wallpaper_service::delete_batch(&ctx.db, req.ids).await?;

    if affected_configs.is_empty() {
        // 场景 1a：没有任何显示器在用这些壁纸，无需联动
        return Ok(deleted);
    }

    // 3. 读取 display_mode 判断同步模式
    let is_sync = playback_cascade::is_sync_mode(&ctx.db).await;

    // 4. 逐个处理受影响的显示器
    let wm = ctx.window_manager.clone();
    let mut sched = scheduler.lock().await;
    let wm_guard = wm.lock().await;

    for config in &affected_configs {
        let mid = &config.monitor_id;
        let key = carousel_key(mid);

        match config.collection_id {
            Some(cid) => {
                // 收藏夹模式：当前壁纸被删，尝试切换到下一张或清空
                playback_cascade::handle_current_wallpaper_removed(
                    &ctx.db, &mut sched, &wm_guard, config, cid, &deleted_ids, is_sync,
                ).await;
            }
            None => {
                // 场景 1b：单张模式，壁纸被删 → 清空窗口 + 停止定时器
                sched.stop(&key);
                wm_guard.notify_wallpaper_cleared(mid, is_sync);
            }
        }
    }

    // 5. 场景 1d：检查未直接受影响的显示器的定时器状态
    let affected_mids: HashSet<String> = affected_configs.iter()
        .map(|c| c.monitor_id.clone())
        .collect();
    playback_cascade::check_orphaned_timers(&ctx.db, &mut sched, &affected_mids).await;

    // 6. 通知主窗口刷新 config 状态
    wm_guard.notify_config_refreshed();

    Ok(deleted)
}
