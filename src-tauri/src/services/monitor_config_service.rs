use anyhow::{Context, Result};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};

use crate::{dto::monitor_config_dto::UpsertMonitorConfigRequest, entities::monitor_config};

/// 获取所有显示器配置
pub async fn get_all(db: &DatabaseConnection) -> Result<Vec<monitor_config::Model>> {
    monitor_config::Entity::find()
        .all(db)
        .await
        .context("Failed to fetch monitor configs")
}

/// 根据 monitor_id 获取配置（唯一）
pub async fn get_by_monitor_id(
    db: &DatabaseConnection,
    monitor_id: &str,
) -> Result<Option<monitor_config::Model>> {
    monitor_config::Entity::find()
        .filter(monitor_config::Column::MonitorId.eq(monitor_id))
        .one(db)
        .await
        .context("Failed to fetch monitor config")
}

/// 创建或更新显示器配置（upsert by monitor_id）
///
/// wallpaper_id: 当前播放的壁纸（始终是实际播放的那张）
/// collection_id: 关联的收藏夹（用于轮播，可与 wallpaper_id 共存）
/// clear_collection: 显式清空 collection_id（切换到单张模式时使用）
/// active: 显示器是否当前物理可用
pub async fn upsert(
    db: &DatabaseConnection,
    req: &UpsertMonitorConfigRequest
) -> Result<monitor_config::Model> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let existing = get_by_monitor_id(db, &req.monitor_id).await?;

    if let Some(existing) = existing {
        // Update
        let mut active_model: monitor_config::ActiveModel = existing.into();

        if let Some(wid) = req.wallpaper_id {
            active_model.wallpaper_id = Set(Some(wid));
        }
        if let Some(cid) = req.collection_id {
            active_model.collection_id = Set(Some(cid));
        }
        // 显式清空 collection_id（从收藏夹模式切回单张壁纸时）
        if req.clear_collection.unwrap_or(false) {
            active_model.collection_id = Set(None);
        }
        if let Some(fm) = &req.fit_mode {
            active_model.fit_mode = Set(fm.to_string());
        }
        if let Some(pm) = &req.play_mode {
            active_model.play_mode = Set(pm.to_string());
        }
        if let Some(pi) = req.play_interval {
            active_model.play_interval = Set(pi);
        }
        if let Some(ie) = req.is_enabled {
            active_model.is_enabled = Set(ie);
        }
        if let Some(a) = req.active {
            active_model.active = Set(a);
        }
        active_model.updated_at = Set(now);

        let model = active_model
            .update(db)
            .await
            .context("Failed to update monitor config")?;
        Ok(model)
    } else {
        // Insert
        let active_model = monitor_config::ActiveModel {
            monitor_id: Set(req.monitor_id.to_string()),
            wallpaper_id: Set(req.wallpaper_id),
            collection_id: Set(req.collection_id),
            fit_mode: Set(req.fit_mode.clone().unwrap_or("cover".to_string())),
            play_mode: Set(req.play_mode.clone().unwrap_or("sequential".to_string())),
            play_interval: Set(req.play_interval.unwrap_or(300)),
            is_enabled: Set(req.is_enabled.unwrap_or(false)),
            active: Set(req.active.unwrap_or(false)),
            updated_at: Set(now),
            ..Default::default()
        };

        let model = active_model
            .insert(db)
            .await
            .context("Failed to insert monitor config")?;
        Ok(model)
    }
}

/// 仅更新 wallpaper_id（定时器内部调用，不触发定时器逻辑）
pub async fn update_wallpaper_id(
    db: &DatabaseConnection,
    monitor_id: &str,
    wallpaper_id: i32,
) -> Result<monitor_config::Model> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let existing = get_by_monitor_id(db, monitor_id)
        .await?
        .context("Monitor config not found")?;

    let mut active_model: monitor_config::ActiveModel = existing.into();
    active_model.wallpaper_id = Set(Some(wallpaper_id));
    active_model.updated_at = Set(now);

    let model = active_model
        .update(db)
        .await
        .context("Failed to update wallpaper_id")?;
    Ok(model)
}

/// 查询引用指定收藏夹的 monitor_id 列表
pub async fn get_monitor_ids_by_collection_id(
    db: &DatabaseConnection,
    collection_id: i32,
) -> Result<Vec<String>> {
    let configs = monitor_config::Entity::find()
        .filter(monitor_config::Column::CollectionId.eq(collection_id))
        .all(db)
        .await
        .context("Failed to query monitor_configs by collection_id")?;

    Ok(configs.into_iter().map(|c| c.monitor_id).collect())
}

/// 查询引用指定壁纸 ID 列表的 monitor_id 列表
pub async fn get_monitor_ids_by_wallpaper_ids(
    db: &DatabaseConnection,
    wallpaper_ids: &[i32],
) -> Result<Vec<String>> {
    let configs = monitor_config::Entity::find()
        .filter(monitor_config::Column::WallpaperId.is_in(wallpaper_ids.iter().copied()))
        .all(db)
        .await
        .context("Failed to query monitor_configs by wallpaper_ids")?;

    Ok(configs.into_iter().map(|c| c.monitor_id).collect())
}

/// 删除显示器配置
pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<()> {
    monitor_config::Entity::delete_by_id(id)
        .exec(db)
        .await
        .context("Failed to delete monitor config")?;
    Ok(())
}

/// 将 source config 的配置（除 id/monitor_id/active 外）同步到目标 monitor
///
/// 用于 mirror/extend 模式下，以基准显示器的配置覆盖从属显示器。
pub async fn sync_config_from(
    db: &DatabaseConnection,
    target_monitor_id: &str,
    source: &monitor_config::Model,
) -> Result<monitor_config::Model> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let existing = get_by_monitor_id(db, target_monitor_id)
        .await?
        .context("Target monitor config not found")?;

    let mut active_model: monitor_config::ActiveModel = existing.into();
    active_model.wallpaper_id = Set(source.wallpaper_id);
    active_model.collection_id = Set(source.collection_id);
    active_model.fit_mode = Set(source.fit_mode.clone());
    active_model.play_mode = Set(source.play_mode.clone());
    active_model.play_interval = Set(source.play_interval);
    active_model.is_enabled = Set(source.is_enabled);
    active_model.updated_at = Set(now);

    let model = active_model
        .update(db)
        .await
        .context("Failed to sync monitor config")?;
    Ok(model)
}