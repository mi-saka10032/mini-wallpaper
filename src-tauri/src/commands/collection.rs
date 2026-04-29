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
use crate::runtime::carousel::carousel_key;
use crate::runtime::Scheduler;
use crate::services::{collection_service, monitor_config_service};

use super::error::CommandResult;
use super::playback_cascade;

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
/// 删除后根据每个受影响显示器的状态执行联动：
/// - 场景 2a：显示器未引用该收藏夹 → 略过
/// - 场景 2b：显示器引用该收藏夹且无 wallpaper_id → 清空壁纸窗口
/// - 场景 2c：显示器引用该收藏夹但有 wallpaper_id → 回退单张模式，停止定时器，壁纸继续显示
/// - 场景 2d：display_mode 同步模式下广播到所有窗口
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

    if affected_configs.is_empty() {
        // 场景 2a：没有任何显示器引用该收藏夹，无需联动
        return Ok(());
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

        // 无论哪种子场景，定时器都需要停止（收藏夹已删除，轮播不再可能）
        sched.stop(&key);

        match config.wallpaper_id {
            Some(_wid) => {
                // 场景 2c：有 wallpaper_id → 回退单张模式，壁纸继续显示，无需通知壁纸窗口
                // collection_id 已被数据层置空，前端刷新 config 后 UI 会自动更新
            }
            None => {
                // 场景 2b：无 wallpaper_id → 清空壁纸窗口
                wm_guard.notify_wallpaper_cleared(mid, is_sync);
            }
        }
    }

    // 5. 通知主窗口刷新 config 状态（collection_id 被置空等变更需要同步到前端 store）
    wm_guard.notify_config_refreshed();

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
/// 移除后根据每个受影响显示器的状态执行联动：
/// - 场景 3a：移除的壁纸不是当前播放的 → 检查剩余数量，≤1 则停止定时器
/// - 场景 3b：移除的壁纸是当前播放的，且剩余 ≥1 张 → 切换到下一张 + 重启定时器
/// - 场景 3c：移除的壁纸是当前播放的，且剩余 0 张 → 清空壁纸窗口
/// - 场景 3d：display_mode 同步模式下广播到所有窗口
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

    if removed == 0 || bound_configs.is_empty() {
        // 没有实际移除或没有显示器绑定该收藏夹，无需联动
        return Ok(removed);
    }

    // 3. 读取 display_mode 判断同步模式
    let is_sync = playback_cascade::is_sync_mode(&ctx.db).await;

    // 4. 逐个处理绑定该收藏夹的显示器
    let wm = ctx.window_manager.clone();
    let mut sched = scheduler.lock().await;
    let wm_guard = wm.lock().await;
    let mut config_changed = false;

    for config in &bound_configs {
        let mid = &config.monitor_id;
        let key = carousel_key(mid);

        let current_wid_removed = config.wallpaper_id
            .map(|wid| removing_ids.contains(&wid))
            .unwrap_or(false);

        if current_wid_removed {
            // 当前播放的壁纸被移除，委托给公共 helper 处理
            let changed = playback_cascade::handle_current_wallpaper_removed(
                &ctx.db, &mut sched, &wm_guard, config, collection_id, &removing_ids, is_sync,
            ).await;
            config_changed = config_changed || changed;
        } else {
            // 场景 3a：当前壁纸未被移除，但收藏夹内容减少了
            // 检查剩余数量，≤1 则停止定时器（轮播无意义）
            if sched.is_running(&key) {
                playback_cascade::manage_timer_by_collection(
                    &ctx.db, &mut sched, mid, collection_id, false,
                ).await;
            }
        }
    }

    // 5. 如果有 config 变更，通知主窗口刷新状态
    if config_changed {
        wm_guard.notify_config_refreshed();
    }

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
