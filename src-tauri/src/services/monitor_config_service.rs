use anyhow::{Context, Result};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    Set, TransactionTrait,
};

use crate::{
    dto::app_setting_dto::keys as setting_keys,
    dto::monitor_config_dto::UpsertMonitorConfigRequest,
    entities::monitor_config,
    services::{app_setting_service, wallpaper_service},
};

/// 校验壁纸可被设为桌面壁纸（存在且不在回收站）
///
/// `monitor_configs.wallpaper_id` 代表「当前真实显示在桌面上的壁纸」，而回收站内的
/// 壁纸在语义上应视为不存在。数据库层面 `deleted_at` 不会阻止此处写入，因此把校验
/// 收归到 service 写入入口，使「回收站壁纸不可上桌」成为后端不变量，而非仅靠前端
/// 视图约定（前端状态滞后、双屏并发、未来新增入口都可能绕过 UI 限制）。
async fn ensure_wallpaper_settable<C: ConnectionTrait>(db: &C, wallpaper_id: i32) -> Result<()> {
    let exists = wallpaper_service::get_active_by_id(db, wallpaper_id)
        .await?
        .is_some();
    if !exists {
        anyhow::bail!(
            "Wallpaper {} is not available (missing or in trash)",
            wallpaper_id
        );
    }
    Ok(())
}

/// 获取所有显示器配置
pub async fn get_all<C: ConnectionTrait>(db: &C) -> Result<Vec<monitor_config::Model>> {
    monitor_config::Entity::find()
        .all(db)
        .await
        .context("Failed to fetch monitor configs")
}

/// 根据 monitor_id 获取配置（唯一）
pub async fn get_by_monitor_id<C: ConnectionTrait>(
    db: &C,
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
pub async fn upsert<C: ConnectionTrait>(
    db: &C,
    req: &UpsertMonitorConfigRequest,
) -> Result<monitor_config::Model> {
    // 写入前守卫：拒绝把不存在或回收站内的壁纸设为桌面壁纸（覆盖 update / insert 两个分支）
    if let Some(wid) = req.wallpaper_id {
        ensure_wallpaper_settable(db, wid).await?;
    }

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
pub async fn update_wallpaper_id<C: ConnectionTrait>(
    db: &C,
    monitor_id: &str,
    wallpaper_id: i32,
) -> Result<monitor_config::Model> {
    // 写入前守卫：轮播定时器持有的 id 可能已被移入回收站，此处兜底拦截
    ensure_wallpaper_settable(db, wallpaper_id).await?;

    let existing = get_by_monitor_id(db, monitor_id)
        .await?
        .context("Monitor config not found")?;

    let mut active_model: monitor_config::ActiveModel = existing.into();
    active_model.wallpaper_id = Set(Some(wallpaper_id));

    let model = active_model
        .update(db)
        .await
        .context("Failed to update wallpaper_id")?;
    Ok(model)
}

/// 清空显示器当前壁纸。收藏夹绑定保持不变。
pub async fn clear_wallpaper_id<C: ConnectionTrait>(
    db: &C,
    monitor_id: &str,
) -> Result<monitor_config::Model> {
    let existing = get_by_monitor_id(db, monitor_id)
        .await?
        .context("Monitor config not found")?;

    let mut active_model: monitor_config::ActiveModel = existing.into();
    active_model.wallpaper_id = Set(None);

    let model = active_model
        .update(db)
        .await
        .context("Failed to clear wallpaper_id")?;
    Ok(model)
}

/// 查询引用指定壁纸 ID 列表的完整 config 列表
///
/// 与 `get_monitor_ids_by_wallpaper_ids` 不同，返回完整 Model，
/// 供删除联动逻辑判断 collection_id / wallpaper_id 等字段。
pub async fn get_configs_by_wallpaper_ids(
    db: &DatabaseConnection,
    wallpaper_ids: &[i32],
) -> Result<Vec<monitor_config::Model>> {
    monitor_config::Entity::find()
        .filter(monitor_config::Column::WallpaperId.is_in(wallpaper_ids.iter().copied()))
        .all(db)
        .await
        .context("Failed to query monitor_configs by wallpaper_ids")
}

/// 查询引用指定收藏夹的完整 config 列表
///
/// 返回完整 Model，供删除联动逻辑判断 wallpaper_id 等字段。
pub async fn get_configs_by_collection_id(
    db: &DatabaseConnection,
    collection_id: i32,
) -> Result<Vec<monitor_config::Model>> {
    monitor_config::Entity::find()
        .filter(monitor_config::Column::CollectionId.eq(collection_id))
        .all(db)
        .await
        .context("Failed to query monitor_configs by collection_id")
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
pub async fn sync_config_from<C: ConnectionTrait>(
    db: &C,
    target_monitor_id: &str,
    source: &monitor_config::Model,
) -> Result<monitor_config::Model> {
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

    let model = active_model
        .update(db)
        .await
        .context("Failed to sync monitor config")?;
    Ok(model)
}

/// 组写入的唯一实现：在一个已开启的事务内，把基准配置全量复制到组内其它屏。
///
/// 三个公开批量函数（update_monitors_batch / upsert_group / apply_group_wallpaper）
/// 只负责各自算出 source 与 targets，事务边界和对齐循环都收在这里，避免重复实现。
/// 返回 [基准配置, 各从属屏配置]；active 是物理状态，sync_config_from 不覆盖。
async fn write_group<C: ConnectionTrait>(
    txn: &C,
    source: monitor_config::Model,
    source_monitor_id: &str,
    targets: &[String],
) -> Result<Vec<monitor_config::Model>> {
    let mut updated = Vec::with_capacity(targets.len().max(1) + 1);
    updated.push(source.clone());
    for id in targets {
        if id == source_monitor_id {
            continue;
        }
        updated.push(sync_config_from(txn, id, &source).await?);
    }
    Ok(updated)
}

/// 在单个事务内把 source 的配置批量同步到所有目标显示器。
///
/// 架构重整 Phase 3：display_mode 切换原本逐条 await `sync_config_from`，
/// 每条各自提交，既无原子性又会让 watcher/reconcile 看到「部分屏已同步」的中间态。
/// 改为一次事务写全组，commit 成功后才由调用方触发一次 reconcile。
/// `encourage` 语义：任何一条失败则整批回滚，DB 保持切换前的状态。
pub async fn update_monitors_batch(
    db: &DatabaseConnection,
    source: &monitor_config::Model,
    target_monitor_ids: &[String],
) -> Result<Vec<monitor_config::Model>> {
    let txn = db.begin().await.context("Failed to begin batch update")?;
    // 本函数的 targets 是「除基准屏外的从属屏」，基准屏本身不参与写入。
    let updated = write_group(&txn, source.clone(), &source.monitor_id, target_monitor_ids).await?;
    txn.commit()
        .await
        .context("Failed to commit batch update")?;
    // 返回值不含基准屏（保持旧语义：调用方只关心被同步的从属屏结果）。
    let mut result = updated;
    result.remove(0);
    Ok(result)
}

/// 同步模式判定：后端唯一口径，避免各入口各写一份字符串比较。
pub fn is_sync_mode(display_mode: &str) -> bool {
    matches!(display_mode, "mirror" | "extend")
}

/// 裁决一次「更新显示器配置」应写入哪些目标。
///
/// 后端单点读取 display_mode：mirror/extend 视为同步组，返回所有 active 显示器
/// （按 monitor_id 排序保证顺序确定）；independent 只返回请求里的那一屏。
/// 前端不再判断 isSyncMode —— 同步语义收归后端，新增入口不可能漏判。
pub async fn active_targets(
    db: &DatabaseConnection,
    monitor_id: &str,
) -> Result<(Vec<String>, bool)> {
    let display_mode = app_setting_service::get(db, setting_keys::DISPLAY_MODE)
        .await?
        .unwrap_or_else(|| "independent".to_string());
    let is_sync = is_sync_mode(&display_mode);
    if !is_sync {
        return Ok((vec![monitor_id.to_string()], false));
    }
    let mut ids: Vec<String> = get_all(db)
        .await?
        .into_iter()
        .filter(|c| c.active)
        .map(|c| c.monitor_id)
        .collect();
    ids.sort();
    // 目标屏尚未登记时（热插拔窗口期）仍要写入，否则该次设置会丢失。
    if !ids.contains(&monitor_id.to_string()) {
        ids.push(monitor_id.to_string());
        ids.sort();
    }
    Ok((ids, true))
}

/// 同步组的一次 upsert：单事务写全组 + 全量配置对齐。
///
/// 顺序固定：先对基准屏应用本次请求，再把更新后的基准配置完整复制到组内其它屏。
/// 全量对齐（wallpaper_id / collection_id / fit_mode / play_mode / play_interval /
/// is_enabled）是兜底：同步组内任一字段不对齐，后续轮播与切换都会分叉。
/// 整个写操作在一个事务内，commit 成功后调用方才触发一次 reconcile，
/// 避免 watcher 看到「部分屏已更新」的中间态。
pub async fn upsert_group(
    db: &DatabaseConnection,
    req: &UpsertMonitorConfigRequest,
) -> Result<(Vec<monitor_config::Model>, bool)> {
    let (targets, is_sync) = active_targets(db, &req.monitor_id).await?;
    let txn = db.begin().await.context("Failed to begin group upsert")?;

    // 基准屏应用本次请求；复用 upsert 保证 settable 守卫与 insert/update 分支一致。
    let source = upsert(&txn, req).await?;
    let updated = write_group(&txn, source, &req.monitor_id, &targets).await?;
    txn.commit()
        .await
        .context("Failed to commit group upsert")?;
    Ok((updated, is_sync))
}

/// 把同一张壁纸（或清空）应用到同步组所有 active 屏，单事务 + 全量对齐。
///
/// `wallpaper_id = None` 表示整组清空显示（删除联动里收藏夹已空的场景）。
/// 与 `upsert_group` 同构：先写基准屏，再把它的完整配置复制到组内其它屏，
/// commit 后由调用方触发一次 reconcile(All)，避免逐屏派发错开帧时钟。
pub async fn apply_group_wallpaper(
    db: &DatabaseConnection,
    source_monitor_id: &str,
    wallpaper_id: Option<i32>,
) -> Result<(Vec<monitor_config::Model>, bool)> {
    let (targets, is_sync) = active_targets(db, source_monitor_id).await?;
    let txn = db
        .begin()
        .await
        .context("Failed to begin group wallpaper")?;

    let source = match wallpaper_id {
        Some(wid) => update_wallpaper_id(&txn, source_monitor_id, wid).await?,
        None => clear_wallpaper_id(&txn, source_monitor_id).await?,
    };
    let updated = write_group(&txn, source, source_monitor_id, &targets).await?;
    txn.commit()
        .await
        .context("Failed to commit group wallpaper")?;
    Ok((updated, is_sync))
}

/// 判断是否满足轮播启动条件（纯逻辑判断，不涉及 DB）
///
/// 仅检查 active + is_enabled 两个开关条件，
/// collection_id 是否存在由调用方在具体场景中额外判断。
pub fn should_start_timer(config: &monitor_config::Model) -> bool {
    config.active && config.is_enabled
}
