//! 收藏夹相关用例：删除收藏夹、移除成员、切换收藏。

use std::collections::HashSet;

use anyhow::Result;

use super::{effects::AppEffects, AppServices};
use crate::services::{collection_service, monitor_config_service};

/// 删除收藏夹，并联动清理绑定它的显示器与窗口。
///
/// 必须在写库前取 config 快照：删除会把 monitor_config.collection_id 置空。
pub async fn delete_collection(svc: &AppServices<'_>, collection_id: i32) -> Result<()> {
    let affected_configs =
        monitor_config_service::get_configs_by_collection_id(&svc.ctx.db, collection_id).await?;
    collection_service::delete(&svc.ctx.db, collection_id).await?;

    let mut guard = svc.scheduler.lock().await;
    let mut fx = AppEffects::new(svc.ctx, &mut guard);
    fx.on_collection_deleted(&affected_configs).await;
    Ok(())
}

/// 从收藏夹移除壁纸，并联动切走正在显示它们的桌面。
pub async fn remove_wallpapers_from_collection(
    svc: &AppServices<'_>,
    collection_id: i32,
    wallpaper_ids: Vec<i32>,
) -> Result<u64> {
    let removing_ids: HashSet<i32> = wallpaper_ids.iter().copied().collect();
    let bound_configs =
        monitor_config_service::get_configs_by_collection_id(&svc.ctx.db, collection_id).await?;

    let removed =
        collection_service::remove_wallpapers(&svc.ctx.db, collection_id, wallpaper_ids).await?;
    if removed == 0 {
        return Ok(removed);
    }

    let mut guard = svc.scheduler.lock().await;
    let mut fx = AppEffects::new(svc.ctx, &mut guard);
    fx.on_wallpapers_removed_from_collection(&bound_configs, collection_id, &removing_ids)
        .await;
    Ok(removed)
}

/// 切换「我喜欢」归属：红心按钮、托盘、全局快捷键共用这一用例。
pub async fn toggle_favorite(svc: &AppServices<'_>, wallpaper_id: i32) -> Result<bool> {
    let mut guard = svc.scheduler.lock().await;
    let mut fx = AppEffects::new(svc.ctx, &mut guard);
    fx.toggle_favorite(wallpaper_id).await
}
