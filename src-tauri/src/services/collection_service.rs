use anyhow::Result;
use sea_orm::prelude::Expr;
use sea_orm::*;

use crate::entities::{collection, collection_wallpaper, monitor_config, wallpaper};

/// 获取所有收藏夹
pub async fn get_all(db: &DatabaseConnection) -> Result<Vec<collection::Model>> {
    let collections = collection::Entity::find()
        .order_by_asc(collection::Column::SortOrder)
        .order_by_asc(collection::Column::Id)
        .all(db)
        .await?;
    Ok(collections)
}

/// 校验收藏夹名称
fn validate_name(name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("收藏夹名称不能为空");
    }
    if name.chars().count() > 32 {
        anyhow::bail!("收藏夹名称不能超过 32 个字符");
    }
    Ok(())
}

/// 创建收藏夹
pub async fn create(db: &DatabaseConnection, name: String) -> Result<collection::Model> {
    validate_name(&name)?;
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let model = collection::ActiveModel {
        name: Set(name),
        sort_order: Set(0),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    };
    let result = collection::Entity::insert(model).exec(db).await?;

    collection::Entity::find_by_id(result.last_insert_id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Failed to find created collection"))
}

/// 重命名收藏夹
pub async fn rename(db: &DatabaseConnection, id: i32, name: String) -> Result<collection::Model> {
    validate_name(&name)?;
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let model = collection::ActiveModel {
        id: Set(id),
        name: Set(name),
        updated_at: Set(now),
        ..Default::default()
    };
    collection::Entity::update(model).exec(db).await?;

    collection::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Collection not found"))
}

/// 删除收藏夹（手动清理关联记录，不依赖外键级联）
pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<()> {
    // 1. 清理 collection_wallpapers 关联记录
    collection_wallpaper::Entity::delete_many()
        .filter(collection_wallpaper::Column::CollectionId.eq(id))
        .exec(db)
        .await?;

    // 2. 清理 monitor_configs 中引用该收藏夹的字段置空
    monitor_config::Entity::update_many()
        .col_expr(
            monitor_config::Column::CollectionId,
            Expr::value(sea_orm::Value::Int(None)),
        )
        .filter(monitor_config::Column::CollectionId.eq(id))
        .exec(db)
        .await?;

    // 3. 删除收藏夹本身
    collection::Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}

/// 获取收藏夹内的壁纸列表
pub async fn get_wallpapers(
    db: &DatabaseConnection,
    collection_id: i32,
) -> Result<Vec<wallpaper::Model>> {
    // 通过关联表查询
    let cw_list = collection_wallpaper::Entity::find()
        .filter(collection_wallpaper::Column::CollectionId.eq(collection_id))
        .order_by_asc(collection_wallpaper::Column::SortOrder)
        .all(db)
        .await?;

    if cw_list.is_empty() {
        return Ok(vec![]);
    }

    let wallpaper_ids: Vec<i32> = cw_list.iter().map(|cw| cw.wallpaper_id).collect();

    let wallpapers = wallpaper::Entity::find()
        .filter(wallpaper::Column::Id.is_in(wallpaper_ids.clone()))
        .all(db)
        .await?;

    // 按关联表的 sort_order 排序
    let mut sorted = Vec::new();
    for id in wallpaper_ids {
        if let Some(wp) = wallpapers.iter().find(|w| w.id == id) {
            sorted.push(wp.clone());
        }
    }

    Ok(sorted)
}

/// 向收藏夹添加壁纸（批量）
pub async fn add_wallpapers(
    db: &DatabaseConnection,
    collection_id: i32,
    wallpaper_ids: Vec<i32>,
) -> Result<u32> {
    let mut count = 0u32;
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    // 查询已有的关联，避免重复插入
    let existing = collection_wallpaper::Entity::find()
        .filter(collection_wallpaper::Column::CollectionId.eq(collection_id))
        .all(db)
        .await?;
    let existing_ids: std::collections::HashSet<i32> =
        existing.iter().map(|cw| cw.wallpaper_id).collect();

    // 计算当前最大 sort_order，新增壁纸追加到末尾
    let max_sort_order = existing.iter().map(|cw| cw.sort_order).max().unwrap_or(-1);
    let mut next_order = max_sort_order + 1;

    for wp_id in wallpaper_ids {
        if existing_ids.contains(&wp_id) {
            continue;
        }
        let model = collection_wallpaper::ActiveModel {
            collection_id: Set(collection_id),
            wallpaper_id: Set(wp_id),
            sort_order: Set(next_order),
            added_at: Set(now.clone()),
        };
        collection_wallpaper::Entity::insert(model)
            .exec(db)
            .await?;
        count += 1;
        next_order += 1;
    }

    Ok(count)
}

/// 重新排序收藏夹内的壁纸
///
/// 接收按新顺序排列的 wallpaper_ids，按索引写入 sort_order
pub async fn reorder_wallpapers(
    db: &DatabaseConnection,
    collection_id: i32,
    wallpaper_ids: Vec<i32>,
) -> Result<()> {
    for (index, wp_id) in wallpaper_ids.iter().enumerate() {
        collection_wallpaper::Entity::update_many()
            .col_expr(
                collection_wallpaper::Column::SortOrder,
                Expr::value(index as i32),
            )
            .filter(collection_wallpaper::Column::CollectionId.eq(collection_id))
            .filter(collection_wallpaper::Column::WallpaperId.eq(*wp_id))
            .exec(db)
            .await?;
    }
    Ok(())
}

/// 从收藏夹移除壁纸（批量）
pub async fn remove_wallpapers(
    db: &DatabaseConnection,
    collection_id: i32,
    wallpaper_ids: Vec<i32>,
) -> Result<u64> {
    let result = collection_wallpaper::Entity::delete_many()
        .filter(collection_wallpaper::Column::CollectionId.eq(collection_id))
        .filter(collection_wallpaper::Column::WallpaperId.is_in(wallpaper_ids))
        .exec(db)
        .await?;
    Ok(result.rows_affected)
}

/// 获取收藏夹内的壁纸数量
pub async fn count_wallpapers(db: &DatabaseConnection, collection_id: i32) -> Result<u64> {
    let count = collection_wallpaper::Entity::find()
        .filter(collection_wallpaper::Column::CollectionId.eq(collection_id))
        .count(db)
        .await?;
    Ok(count)
}

/// 根据 play_mode 获取收藏夹中的下一张壁纸 ID
///
/// - sequential: 按 sort_order 取当前 wallpaper_id 的下一张，末尾回首
/// - random: SQLite RANDOM() 随机取一张（排除当前）
pub async fn next_wallpaper_id(
    db: &DatabaseConnection,
    collection_id: i32,
    current_wallpaper_id: Option<i32>,
    play_mode: &str,
) -> Result<Option<i32>> {
    use sea_orm::{ConnectionTrait, Statement};

    match play_mode {
        "random" => {
            // 随机取一张，排除当前
            let sql = if let Some(cwid) = current_wallpaper_id {
                format!(
                    "SELECT wallpaper_id FROM collection_wallpapers
                     WHERE collection_id = {} AND wallpaper_id != {}
                     ORDER BY RANDOM() LIMIT 1",
                    collection_id, cwid
                )
            } else {
                format!(
                    "SELECT wallpaper_id FROM collection_wallpapers
                     WHERE collection_id = {}
                     ORDER BY RANDOM() LIMIT 1",
                    collection_id
                )
            };

            let result = db
                .query_one(Statement::from_string(
                    sea_orm::DatabaseBackend::Sqlite,
                    sql,
                ))
                .await?;

            if let Some(row) = result {
                let wid: i32 = row.try_get("", "wallpaper_id")?;
                Ok(Some(wid))
            } else {
                // 只有一张时 fallback 取第一张
                first_wallpaper_id(db, collection_id).await
            }
        }
        _ => {
            // sequential: 找当前 wallpaper_id 在 sort_order 中的位置，取下一张
            if let Some(cwid) = current_wallpaper_id {
                let current_row = db
                    .query_one(Statement::from_string(
                        sea_orm::DatabaseBackend::Sqlite,
                        format!(
                            "SELECT sort_order FROM collection_wallpapers
                             WHERE collection_id = {} AND wallpaper_id = {}",
                            collection_id, cwid
                        ),
                    ))
                    .await?;

                if let Some(row) = current_row {
                    let current_order: i32 = row.try_get("", "sort_order")?;

                    // 取 sort_order 大于当前的下一张
                    let next = db
                        .query_one(Statement::from_string(
                            sea_orm::DatabaseBackend::Sqlite,
                            format!(
                                "SELECT wallpaper_id FROM collection_wallpapers
                                 WHERE collection_id = {} AND sort_order > {}
                                 ORDER BY sort_order ASC LIMIT 1",
                                collection_id, current_order
                            ),
                        ))
                        .await?;

                    if let Some(row) = next {
                        let wid: i32 = row.try_get("", "wallpaper_id")?;
                        return Ok(Some(wid));
                    }
                }
            }

            // fallback: 当前壁纸不在收藏夹中 / 已到末尾 → 回到第一张
            first_wallpaper_id(db, collection_id).await
        }
    }
}

/// 获取收藏夹中 sort_order 最小的第一张壁纸 ID
async fn first_wallpaper_id(
    db: &DatabaseConnection,
    collection_id: i32,
) -> Result<Option<i32>> {
    use sea_orm::{ConnectionTrait, Statement};

    let result = db
        .query_one(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            format!(
                "SELECT wallpaper_id FROM collection_wallpapers
                 WHERE collection_id = {} ORDER BY sort_order ASC LIMIT 1",
                collection_id
            ),
        ))
        .await?;

    Ok(result
        .and_then(|r| r.try_get::<i32>("", "wallpaper_id").ok()))
}

/// 获取收藏夹中 sort_order 最大的最后一张壁纸 ID
async fn last_wallpaper_id(
    db: &DatabaseConnection,
    collection_id: i32,
) -> Result<Option<i32>> {
    use sea_orm::{ConnectionTrait, Statement};

    let result = db
        .query_one(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            format!(
                "SELECT wallpaper_id FROM collection_wallpapers
                 WHERE collection_id = {} ORDER BY sort_order DESC LIMIT 1",
                collection_id
            ),
        ))
        .await?;

    Ok(result
        .and_then(|r| r.try_get::<i32>("", "wallpaper_id").ok()))
}

/// 根据 play_mode 获取收藏夹中的上一张壁纸 ID
///
/// - sequential: 按 sort_order 取当前 wallpaper_id 的前一张，首部回末尾
/// - random: 同 next（随机取一张排除当前）
pub async fn prev_wallpaper_id(
    db: &DatabaseConnection,
    collection_id: i32,
    current_wallpaper_id: Option<i32>,
    play_mode: &str,
) -> Result<Option<i32>> {
    use sea_orm::{ConnectionTrait, Statement};

    match play_mode {
        "random" => {
            // 随机模式下上一张等同于随机取一张
            next_wallpaper_id(db, collection_id, current_wallpaper_id, play_mode).await
        }
        _ => {
            // sequential: 找当前 wallpaper_id 在 sort_order 中的位置，取前一张
            if let Some(cwid) = current_wallpaper_id {
                let current_row = db
                    .query_one(Statement::from_string(
                        sea_orm::DatabaseBackend::Sqlite,
                        format!(
                            "SELECT sort_order FROM collection_wallpapers
                             WHERE collection_id = {} AND wallpaper_id = {}",
                            collection_id, cwid
                        ),
                    ))
                    .await?;

                if let Some(row) = current_row {
                    let current_order: i32 = row.try_get("", "sort_order")?;

                    // 取 sort_order 小于当前的上一张
                    let prev = db
                        .query_one(Statement::from_string(
                            sea_orm::DatabaseBackend::Sqlite,
                            format!(
                                "SELECT wallpaper_id FROM collection_wallpapers
                                 WHERE collection_id = {} AND sort_order < {}
                                 ORDER BY sort_order DESC LIMIT 1",
                                collection_id, current_order
                            ),
                        ))
                        .await?;

                    if let Some(row) = prev {
                        let wid: i32 = row.try_get("", "wallpaper_id")?;
                        return Ok(Some(wid));
                    }
                }
            }

            // fallback: 当前壁纸不在收藏夹中 / 已到首部 → 回到最后一张
            last_wallpaper_id(db, collection_id).await
        }
    }
}
