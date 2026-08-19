//! 标签服务
//!
//! 规范化标签的读写核心，服务于「壁纸打标签」与「智能收藏夹规则」两个上层：
//!
//! - **resolve-or-create**：标签自由输入、不给固定选项。确认时按 name 命中取 id，
//!   不存在则先建标签再取 id（依赖 `tags.name` 唯一索引，见 m008）。
//! - **打标签 / 取消**：`wallpaper_tags` join 表的幂等 add / remove。
//! - **标签列举**：全量标签（带引用计数）供管理 UI；单壁纸标签供卡片回显。
//!
//! ## 与智能收藏夹的衔接
//!
//! 规则中标签条件存 **tag id 数组**（非 name 字符串）：标签改名不影响规则、
//! 查询不进裸字符串。前端在保存规则前调用 [`resolve_or_create_many`]
//! 把用户输入的标签名批量换成 id。

use std::collections::HashSet;

use anyhow::Result;
use sea_orm::prelude::Expr;
use sea_orm::sea_query::Query;
use sea_orm::*;
use serde::Serialize;

use crate::entities::{tag, wallpaper, wallpaper_tag};

/// 标签及其引用计数（供管理 UI 展示「某标签被 N 张壁纸使用」）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagWithCount {
    pub id: i32,
    pub name: String,
    pub created_at: String,
    /// 引用该标签的壁纸数量
    pub wallpaper_count: i64,
}

fn now_str() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// 获取全部标签（按引用计数降序、name 升序），带引用计数
pub async fn get_all_with_count(db: &DatabaseConnection) -> Result<Vec<TagWithCount>> {
    let tags = tag::Entity::find()
        .order_by_asc(tag::Column::Name)
        .all(db)
        .await?;

    let mut out = Vec::with_capacity(tags.len());
    for t in tags {
        // 排除回收站内的壁纸：wallpaper_tags 关联行在软删除时保留（以便恢复后
        // 标签不丢），故计数必须显式过滤，否则标签数会虚高。
        let count = wallpaper_tag::Entity::find()
            .filter(wallpaper_tag::Column::TagId.eq(t.id))
            .filter(
                wallpaper_tag::Column::WallpaperId.in_subquery(
                    sea_orm::sea_query::Query::select()
                        .column(wallpaper::Column::Id)
                        .from(wallpaper::Entity)
                        .and_where(wallpaper::Column::DeletedAt.is_null())
                        .to_owned(),
                ),
            )
            .count(db)
            .await? as i64;
        out.push(TagWithCount {
            id: t.id,
            name: t.name,
            created_at: t.created_at,
            wallpaper_count: count,
        });
    }
    // 引用多的排前面，其次按 name
    out.sort_by(|a, b| {
        b.wallpaper_count
            .cmp(&a.wallpaper_count)
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(out)
}

/// 查某壁纸的全部标签（按 name 升序）
pub async fn get_wallpaper_tags(
    db: &DatabaseConnection,
    wallpaper_id: i32,
) -> Result<Vec<tag::Model>> {
    let tags = tag::Entity::find()
        .filter(
            Condition::any().add(
                tag::Column::Id.in_subquery(
                    Query::select()
                        .column(wallpaper_tag::Column::TagId)
                        .from(wallpaper_tag::Entity)
                        .and_where(Expr::col(wallpaper_tag::Column::WallpaperId).eq(wallpaper_id))
                        .to_owned(),
                ),
            ),
        )
        .order_by_asc(tag::Column::Name)
        .all(db)
        .await?;
    Ok(tags)
}

/// resolve-or-create 单个标签名 → tag id
///
/// name 先 trim；命中已有标签取其 id，否则插入新标签。空白名视为非法。
pub async fn resolve_or_create(db: &DatabaseConnection, name: &str) -> Result<i32> {
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("标签名不能为空");
    }

    if let Some(existing) = tag::Entity::find()
        .filter(tag::Column::Name.eq(name))
        .one(db)
        .await?
    {
        return Ok(existing.id);
    }

    let model = tag::ActiveModel {
        name: Set(name.to_string()),
        created_at: Set(now_str()),
        ..Default::default()
    };
    let res = tag::Entity::insert(model).exec(db).await?;
    Ok(res.last_insert_id)
}

/// resolve-or-create 批量标签名 → tag id 列表（去重、保序）
///
/// 供智能收藏夹规则保存前把标签名换成 id 用。
pub async fn resolve_or_create_many(
    db: &DatabaseConnection,
    names: Vec<String>,
) -> Result<Vec<i32>> {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    for name in names {
        let id = resolve_or_create(db, &name).await?;
        if seen.insert(id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// 给一批壁纸打上一批标签（笛卡尔积，幂等）
///
/// 标签以 name 传入，内部 resolve-or-create 成 id 后写入 join 表；
/// 已存在的 (wallpaper_id, tag_id) 组合跳过。返回新增关联条数。
pub async fn tag_wallpapers(
    db: &DatabaseConnection,
    wallpaper_ids: Vec<i32>,
    tag_names: Vec<String>,
) -> Result<u64> {
    let tag_ids = resolve_or_create_many(db, tag_names).await?;
    if tag_ids.is_empty() || wallpaper_ids.is_empty() {
        return Ok(0);
    }

    let txn = db.begin().await?;
    let now = now_str();
    let mut added = 0u64;

    for &wid in &wallpaper_ids {
        // 该壁纸已有的 tag_id 集合，避免重复插入
        let existing: HashSet<i32> = wallpaper_tag::Entity::find()
            .filter(wallpaper_tag::Column::WallpaperId.eq(wid))
            .all(&txn)
            .await?
            .into_iter()
            .map(|m| m.tag_id)
            .collect();

        for &tid in &tag_ids {
            if existing.contains(&tid) {
                continue;
            }
            let model = wallpaper_tag::ActiveModel {
                wallpaper_id: Set(wid),
                tag_id: Set(tid),
                created_at: Set(now.clone()),
            };
            wallpaper_tag::Entity::insert(model).exec(&txn).await?;
            added += 1;
        }
    }

    txn.commit().await?;
    Ok(added)
}

/// 从一批壁纸移除一批标签（按 tag id）。返回删除关联条数。
pub async fn untag_wallpapers(
    db: &DatabaseConnection,
    wallpaper_ids: Vec<i32>,
    tag_ids: Vec<i32>,
) -> Result<u64> {
    if wallpaper_ids.is_empty() || tag_ids.is_empty() {
        return Ok(0);
    }
    let res = wallpaper_tag::Entity::delete_many()
        .filter(wallpaper_tag::Column::WallpaperId.is_in(wallpaper_ids))
        .filter(wallpaper_tag::Column::TagId.is_in(tag_ids))
        .exec(db)
        .await?;
    Ok(res.rows_affected)
}

/// 覆盖式设置单张壁纸的标签集合（用于卡片标签编辑「保存」）
///
/// 传入目标标签名全集，内部 diff：新增缺失的关联、删除多余的关联。
/// 返回设置后的完整标签列表。
pub async fn set_wallpaper_tags(
    db: &DatabaseConnection,
    wallpaper_id: i32,
    tag_names: Vec<String>,
) -> Result<Vec<tag::Model>> {
    let target_ids: HashSet<i32> =
        resolve_or_create_many(db, tag_names).await?.into_iter().collect();

    let current_ids: HashSet<i32> = wallpaper_tag::Entity::find()
        .filter(wallpaper_tag::Column::WallpaperId.eq(wallpaper_id))
        .all(db)
        .await?
        .into_iter()
        .map(|m| m.tag_id)
        .collect();

    let to_add: Vec<i32> = target_ids.difference(&current_ids).copied().collect();
    let to_remove: Vec<i32> = current_ids.difference(&target_ids).copied().collect();

    let txn = db.begin().await?;
    let now = now_str();

    if !to_remove.is_empty() {
        wallpaper_tag::Entity::delete_many()
            .filter(wallpaper_tag::Column::WallpaperId.eq(wallpaper_id))
            .filter(wallpaper_tag::Column::TagId.is_in(to_remove))
            .exec(&txn)
            .await?;
    }

    for tid in to_add {
        let model = wallpaper_tag::ActiveModel {
            wallpaper_id: Set(wallpaper_id),
            tag_id: Set(tid),
            created_at: Set(now.clone()),
        };
        wallpaper_tag::Entity::insert(model).exec(&txn).await?;
    }

    txn.commit().await?;

    get_wallpaper_tags(db, wallpaper_id).await
}

/// 删除一个标签（连带清理其在 join 表中的全部关联，事务保护）
///
/// 智能收藏夹规则里若引用了被删标签的 id，命中集会自动少掉该维度约束的这一项——
/// 规则依然合法（引擎按 id 求值，查不到即无匹配），无需额外级联处理规则本身。
pub async fn delete_tag(db: &DatabaseConnection, tag_id: i32) -> Result<()> {
    let txn = db.begin().await?;
    wallpaper_tag::Entity::delete_many()
        .filter(wallpaper_tag::Column::TagId.eq(tag_id))
        .exec(&txn)
        .await?;
    tag::Entity::delete_by_id(tag_id).exec(&txn).await?;
    txn.commit().await?;
    Ok(())
}

/// 重命名标签（name 唯一约束下，撞名返回错误）
pub async fn rename_tag(db: &DatabaseConnection, tag_id: i32, new_name: String) -> Result<tag::Model> {
    let new_name = new_name.trim().to_string();
    if new_name.is_empty() {
        anyhow::bail!("标签名不能为空");
    }
    let model = tag::ActiveModel {
        id: Set(tag_id),
        name: Set(new_name),
        ..Default::default()
    };
    tag::Entity::update(model).exec(db).await?;
    tag::Entity::find_by_id(tag_id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Tag not found"))
}

/// 校验一批 tag id 是否都存在（供智能收藏夹规则保存时兜底校验）
#[allow(dead_code)]
pub async fn all_tag_ids_exist(db: &DatabaseConnection, ids: &[i32]) -> Result<bool> {
    if ids.is_empty() {
        return Ok(true);
    }
    let count = tag::Entity::find()
        .filter(tag::Column::Id.is_in(ids.to_vec()))
        .count(db)
        .await?;
    Ok(count as usize == ids.iter().collect::<HashSet<_>>().len())
}

/// 便于其他 service 复用：确保 wallpaper 存在（打标签前的存在性兜底）
///
/// 回收站内的壁纸视为不存在——不允许对已删除的壁纸打标签。
#[allow(dead_code)]
pub async fn wallpaper_exists(db: &DatabaseConnection, wallpaper_id: i32) -> Result<bool> {
    let count = crate::services::wallpaper_service::active()
        .filter(wallpaper::Column::Id.eq(wallpaper_id))
        .count(db)
        .await?;
    Ok(count > 0)
}