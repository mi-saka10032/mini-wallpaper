use anyhow::Result;
use sea_orm::prelude::Expr;
use sea_orm::*;

use crate::dto::shortcut_dto::Direction as SwitchDirection;
use crate::entities::{collection, collection_wallpaper, monitor_config, wallpaper};
use crate::services::wallpaper_service;

/// 标记：业务尝试修改/删除内置收藏夹时返回的错误信息
///
/// 该字符串会经 `CommandResult` 序列化给前端，前端可据此判断是否需要提示
/// "系统收藏夹不可删除/重命名"。
const ERR_BUILTIN_IMMUTABLE: &str = "Builtin collection is immutable";

/// 获取所有收藏夹
pub async fn get_all(db: &DatabaseConnection) -> Result<Vec<collection::Model>> {
    let collections = collection::Entity::find()
        .order_by_asc(collection::Column::SortOrder)
        .order_by_asc(collection::Column::Id)
        .all(db)
        .await?;
    Ok(collections)
}

/// 查找系统内置收藏夹（is_builtin = 1）
///
/// 由 m006 迁移种入，全表至多一条（partial unique index 兜底）。
/// 业务代码（如 ToggleFavorite 快捷键、未来的红心按钮）必须通过本函数动态定位
/// 内置收藏夹 id，**禁止 hardcode `id = 1`**：老用户的 id=1 通常已被既有
/// 收藏夹占用，自增分配的内置收藏夹 id 无法预测。
pub async fn find_builtin(db: &DatabaseConnection) -> Result<Option<collection::Model>> {
    let model = collection::Entity::find()
        .filter(collection::Column::IsBuiltin.eq(1))
        .one(db)
        .await?;
    Ok(model)
}

/// 创建收藏夹
pub async fn create(db: &DatabaseConnection, name: String) -> Result<collection::Model> {
    // 走 ActiveModel::insert，触发 before_save 自动填时间戳
    let model = collection::ActiveModel {
        name: Set(name),
        sort_order: Set(0),
        is_builtin: Set(0),
        ..Default::default()
    };
    let model = model.insert(db).await?;

    collection::Entity::find_by_id(model.id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Failed to find created collection"))
}

/// 创建智能收藏夹
///
/// `rule_json` 必须先通过规则引擎校验（白名单 + 至少一条规则）方可入库，
/// 绝不存裸类 SQL。成员不物化，由 `rule_json` 实时求值。
pub async fn create_smart(
    db: &DatabaseConnection,
    name: String,
    rule_json: String,
) -> Result<collection::Model> {
    // 校验规则合法性（非法直接拒绝，不落库）
    crate::services::smart_rule::SmartRule::parse_and_validate(&rule_json)?;

    // 走 ActiveModel::insert，触发 before_save 自动填时间戳
    let model = collection::ActiveModel {
        name: Set(name),
        sort_order: Set(0),
        is_builtin: Set(0),
        kind: Set("smart".to_string()),
        rule_json: Set(Some(rule_json)),
        ..Default::default()
    };
    let model = model.insert(db).await?;

    collection::Entity::find_by_id(model.id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Failed to find created smart collection"))
}

/// 更新智能收藏夹的规则（可同时改名）
///
/// 守卫：目标必须是 smart 收藏夹；新规则须通过白名单校验。
pub async fn update_smart(
    db: &DatabaseConnection,
    id: i32,
    name: Option<String>,
    rule_json: String,
) -> Result<collection::Model> {
    let existing = collection::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Collection not found"))?;
    if existing.kind != "smart" {
        return Err(anyhow::anyhow!("目标不是智能收藏夹，不能更新规则"));
    }

    crate::services::smart_rule::SmartRule::parse_and_validate(&rule_json)?;

    let mut model = collection::ActiveModel {
        id: Set(id),
        rule_json: Set(Some(rule_json)),
        ..Default::default()
    };
    if let Some(n) = name {
        model.name = Set(n);
    }
    // 走 ActiveModel::update，触发 before_save 刷新 updated_at
    model.update(db).await?;

    collection::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Collection not found"))
}

/// 预览一段规则 JSON 的当前命中数（未落库，供创建 / 编辑时实时提示）
pub async fn preview_smart_count(db: &DatabaseConnection, rule_json: String) -> Result<u64> {
    let rule = crate::services::smart_rule::SmartRule::parse_and_validate(&rule_json)?;
    crate::services::smart_collection_service::count_matched_by_rule(db, &rule).await
}

/// 重命名收藏夹
///
/// 内置收藏夹（is_builtin = 1）不可重命名，会返回错误。
pub async fn rename(db: &DatabaseConnection, id: i32, name: String) -> Result<collection::Model> {
    // 守卫：内置收藏夹不可重命名
    if let Some(existing) = collection::Entity::find_by_id(id).one(db).await? {
        if existing.is_builtin == 1 {
            return Err(anyhow::anyhow!(ERR_BUILTIN_IMMUTABLE));
        }
    }

    let model = collection::ActiveModel {
        id: Set(id),
        name: Set(name),
        ..Default::default()
    };
    // 走 ActiveModel::update，触发 before_save 刷新 updated_at
    model.update(db).await?;

    collection::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Collection not found"))
}

/// 删除收藏夹（手动清理关联记录，不依赖外键级联，事务保护）
///
/// 内置收藏夹（is_builtin = 1）不可删除，会返回错误。
pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<()> {
    // 守卫：内置收藏夹不可删除（前置校验，避免开启事务后再回滚的浪费）
    if let Some(existing) = collection::Entity::find_by_id(id).one(db).await? {
        if existing.is_builtin == 1 {
            return Err(anyhow::anyhow!(ERR_BUILTIN_IMMUTABLE));
        }
    }

    let txn = db.begin().await?;

    // 1. 清理 collection_wallpapers 关联记录
    collection_wallpaper::Entity::delete_many()
        .filter(collection_wallpaper::Column::CollectionId.eq(id))
        .exec(&txn)
        .await?;

    // 2. 清理 monitor_configs 中引用该收藏夹的字段置空
    monitor_config::Entity::update_many()
        .col_expr(
            monitor_config::Column::CollectionId,
            Expr::value(sea_orm::Value::Int(None)),
        )
        .filter(monitor_config::Column::CollectionId.eq(id))
        .exec(&txn)
        .await?;

    // 3. 删除收藏夹本身
    collection::Entity::delete_by_id(id).exec(&txn).await?;

    txn.commit().await?;
    Ok(())
}

/// 重新排序收藏夹列表（事务保护）
///
/// 内置收藏夹（is_builtin = 1）固定占据 sort_order = 0，不参与排序；
/// 用户夹按传入顺序分配 1..n。请求必须恰好包含全部用户夹 id——既防止
/// 漏传导致部分夹 id 停留在旧 order 产生重复，也防止传入内置夹 id 越权。
/// 与 reorder_wallpapers 一致做不动点 diff：order 未变的夹不写库。
pub async fn reorder_collections(db: &DatabaseConnection, collection_ids: Vec<i32>) -> Result<()> {
    let all = collection::Entity::find().all(db).await?;
    let (builtin, custom): (Vec<_>, Vec<_>) = all.into_iter().partition(|m| m.is_builtin == 1);

    let custom_ids: std::collections::HashSet<i32> = custom.iter().map(|m| m.id).collect();
    let incoming: std::collections::HashSet<i32> = collection_ids.iter().copied().collect();

    // 完整性校验：传入集合必须恰好等于全部用户夹集合，且不重复。
    // HashSet 的 != 会先去重，单靠它无法发现重复 id——重复会让下游
    // enumerate() 对同一 id 写入两个 order，造成 sort_order 冲突。
    if incoming != custom_ids || collection_ids.len() != custom_ids.len() {
        anyhow::bail!("Collection order must contain exactly all user collections, no duplicates");
    }

    let txn = db.begin().await?;

    // 内置夹钉在 0（若历史数据非 0 也顺带纠正，保证顶部唯一）
    for m in &builtin {
        if m.sort_order != 0 {
            let mut active: collection::ActiveModel = m.clone().into();
            active.sort_order = Set(0);
            active.update(&txn).await?;
        }
    }

    // 不动点 diff：仅写 sort_order 实际变化的用户夹
    let current: std::collections::HashMap<i32, i32> =
        custom.iter().map(|m| (m.id, m.sort_order)).collect();
    for (index, cid) in collection_ids.iter().enumerate() {
        let new_order = index as i32 + 1;
        if current.get(cid) == Some(&new_order) {
            continue;
        }
        // 单条更新走 ActiveModel::update 以触发 before_save 刷新 updated_at；
        // NotSet 字段（含 id 之外的全部列）会被跳过，只写 sort_order + updated_at。
        let active: collection::ActiveModel = collection::ActiveModel {
            id: Set(*cid),
            sort_order: Set(new_order),
            ..Default::default()
        };
        active.update(&txn).await?;
    }

    txn.commit().await?;
    Ok(())
}

/// 获取收藏夹内的壁纸列表
pub async fn get_wallpapers(
    db: &DatabaseConnection,
    collection_id: i32,
) -> Result<Vec<wallpaper::Model>> {
    // 智能收藏夹：委托规则引擎实时求值（不物化成员）
    if let Some(model) = collection::Entity::find_by_id(collection_id)
        .one(db)
        .await?
    {
        if model.kind == "smart" {
            return crate::services::smart_collection_service::matched_wallpapers(db, &model).await;
        }
    }

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

    let wallpapers = wallpaper_service::active()
        .filter(wallpaper::Column::Id.is_in(wallpaper_ids.iter().copied()))
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

/// 向收藏夹添加壁纸（批量，事务保护）
///
/// 只接受已存在的手动收藏夹，以及未进回收站的壁纸。智能收藏夹的成员由规则
/// 求值，不写入关联表。已有关联跳过，不重复插入。
pub async fn add_wallpapers(
    db: &DatabaseConnection,
    collection_id: i32,
    wallpaper_ids: Vec<i32>,
) -> Result<u32> {
    let txn = db.begin().await?;

    let collection = collection::Entity::find_by_id(collection_id)
        .one(&txn)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Collection not found"))?;
    if collection.kind != "manual" {
        return Err(anyhow::anyhow!(
            "Only manual collections accept wallpaper membership"
        ));
    }

    if !wallpaper_ids.is_empty() {
        let active_ids: std::collections::HashSet<i32> = wallpaper_service::active()
            .filter(wallpaper::Column::Id.is_in(wallpaper_ids.iter().copied()))
            .all(&txn)
            .await?
            .into_iter()
            .map(|wallpaper| wallpaper.id)
            .collect();
        if let Some(missing_id) = wallpaper_ids.iter().find(|id| !active_ids.contains(id)) {
            return Err(anyhow::anyhow!(
                "Wallpaper {} is not available (missing or in trash)",
                missing_id
            ));
        }
    }

    let mut count = 0u32;

    // 查询已有的关联，避免重复插入
    let existing = collection_wallpaper::Entity::find()
        .filter(collection_wallpaper::Column::CollectionId.eq(collection_id))
        .all(&txn)
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
            ..Default::default()
        };
        model.insert(&txn).await?;
        count += 1;
        next_order += 1;
    }

    txn.commit().await?;
    Ok(count)
}

/// 重新排序收藏夹内的壁纸（事务保护）
///
/// 接收按新顺序排列的 wallpaper_ids，按索引写入 sort_order。
///
/// 前端传入的数组只含可见成员（回收站内的已被过滤），而回收站成员的关联行
/// 仍在表中、其 sort_order 为哨兵 `-1`。此处按索引写 `0..n-1` 不会与哨兵冲突，
/// 且显式排除哨兵行，避免把回收站成员误拉回可见序列。
///
/// 只更新 sort_order 实际变化的成员（不动点 diff）：先一次查询取当前顺序，
/// 逐项比对新序，跳过 order 未变的 id，避免对全量成员逐条发 UPDATE。
pub async fn reorder_wallpapers(
    db: &DatabaseConnection,
    collection_id: i32,
    wallpaper_ids: Vec<i32>,
) -> Result<()> {
    let txn = db.begin().await?;

    // 一次查出当前可见成员的 (wallpaper_id -> sort_order)，哨兵行不参与
    let current: std::collections::HashMap<i32, i32> = collection_wallpaper::Entity::find()
        .filter(collection_wallpaper::Column::CollectionId.eq(collection_id))
        .filter(collection_wallpaper::Column::SortOrder.ne(wallpaper_service::TRASH_SORT_ORDER))
        .all(&txn)
        .await?
        .into_iter()
        .map(|row| (row.wallpaper_id, row.sort_order))
        .collect();

    for (index, wp_id) in wallpaper_ids.iter().enumerate() {
        let new_order = index as i32;
        // 不动点：sort_order 未变则跳过，不写库
        if current.get(wp_id) == Some(&new_order) {
            continue;
        }
        collection_wallpaper::Entity::update_many()
            .col_expr(
                collection_wallpaper::Column::SortOrder,
                Expr::value(new_order),
            )
            .filter(collection_wallpaper::Column::CollectionId.eq(collection_id))
            .filter(collection_wallpaper::Column::WallpaperId.eq(*wp_id))
            .filter(collection_wallpaper::Column::SortOrder.ne(wallpaper_service::TRASH_SORT_ORDER))
            .exec(&txn)
            .await?;
    }

    txn.commit().await?;
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
    // 智能收藏夹：命中数由规则参数化 COUNT 求值
    if let Some(model) = collection::Entity::find_by_id(collection_id)
        .one(db)
        .await?
    {
        if model.kind == "smart" {
            return crate::services::smart_collection_service::count_matched(db, &model).await;
        }
    }
    let count = collection_wallpaper::Entity::find()
        .filter(collection_wallpaper::Column::CollectionId.eq(collection_id))
        .filter(active_member_filter())
        .count(db)
        .await?;
    Ok(count)
}

/// 判断某壁纸是否已在指定收藏夹中
///
/// 用于 ToggleFavorite / 红心按钮的"切换"语义：先探测归属，再决定 add / remove。
///
/// 刻意按「关联表有无记录」判断，不加 `active_member_filter`：若把已软删除的
/// 壁纸判为未收藏，切换就会走 add 分支，而 `add_wallpapers` 有 active 守卫，
/// 会直接报 "not available (missing or in trash)"；走 remove 反而能顺手清理
/// 掉残留关系。真正在意软删除的是列表查询，不是这里的归属探测。
pub async fn is_wallpaper_in_collection(
    db: &DatabaseConnection,
    collection_id: i32,
    wallpaper_id: i32,
) -> Result<bool> {
    let count = collection_wallpaper::Entity::find()
        .filter(collection_wallpaper::Column::CollectionId.eq(collection_id))
        .filter(collection_wallpaper::Column::WallpaperId.eq(wallpaper_id))
        .count(db)
        .await?;
    Ok(count > 0)
}

/// 检查收藏夹壁纸数量是否 > 1（满足轮播切换的最低条件）
pub async fn has_enough_wallpapers(db: &DatabaseConnection, collection_id: i32) -> Result<bool> {
    let count = count_wallpapers(db, collection_id).await?;
    Ok(count > 1)
}

// ==================== 可复用的 ORM 辅助函数 ====================

/// 「成员壁纸未进回收站」条件（子查询）
///
/// 手动收藏夹的成员关系存放在 `collection_wallpapers`，该表本身不含删除标记。
/// 壁纸移入回收站时其关联行会被**保留**（以便恢复后仍属于原收藏夹），因此所有
/// 面向播放/展示的成员查询都必须额外排除「关联仍在、但壁纸已进回收站」的行，
/// 否则轮播会切到回收站内的壁纸、数量统计也会虚高。
///
/// 用 `wallpaper_id IN (SELECT id FROM wallpapers WHERE deleted_at IS NULL)`
/// 表达，避免为 6 个游标辅助函数逐一改写 join。
fn active_member_filter() -> Condition {
    Condition::all().add(
        collection_wallpaper::Column::WallpaperId.in_subquery(
            sea_orm::sea_query::Query::select()
                .column(wallpaper::Column::Id)
                .from(wallpaper::Entity)
                .and_where(wallpaper::Column::DeletedAt.is_null())
                .to_owned(),
        ),
    )
}

/// 查询某壁纸在收藏夹中的 sort_order
async fn find_sort_order(
    db: &DatabaseConnection,
    collection_id: i32,
    wallpaper_id: i32,
) -> Result<Option<i32>> {
    let record = collection_wallpaper::Entity::find()
        .filter(collection_wallpaper::Column::CollectionId.eq(collection_id))
        .filter(collection_wallpaper::Column::WallpaperId.eq(wallpaper_id))
        .one(db)
        .await?;
    Ok(record.map(|r| r.sort_order))
}

/// 按 sort_order 边界条件查找壁纸 ID
///
/// - `order_bound > 0`：查找 sort_order > order_bound 的最小值（下一张）
/// - `order_bound < 0`：查找 sort_order < |order_bound| 的最大值（上一张）
/// - 内部通过 `direction` 参数区分方向
async fn find_adjacent_wallpaper(
    db: &DatabaseConnection,
    collection_id: i32,
    current_order: i32,
    direction: Direction,
) -> Result<Option<i32>> {
    let query = match direction {
        Direction::Next => collection_wallpaper::Entity::find()
            .filter(collection_wallpaper::Column::CollectionId.eq(collection_id))
            .filter(collection_wallpaper::Column::SortOrder.gt(current_order))
            .filter(active_member_filter())
            .order_by_asc(collection_wallpaper::Column::SortOrder),
        Direction::Prev => collection_wallpaper::Entity::find()
            .filter(collection_wallpaper::Column::CollectionId.eq(collection_id))
            .filter(collection_wallpaper::Column::SortOrder.lt(current_order))
            .filter(active_member_filter())
            .order_by_desc(collection_wallpaper::Column::SortOrder),
    };

    let record = query.one(db).await?;
    Ok(record.map(|r| r.wallpaper_id))
}

/// 随机取一张壁纸 ID（可排除指定 ID）
async fn find_random_wallpaper(
    db: &DatabaseConnection,
    collection_id: i32,
    exclude_id: Option<i32>,
) -> Result<Option<i32>> {
    let mut query = collection_wallpaper::Entity::find()
        .filter(collection_wallpaper::Column::CollectionId.eq(collection_id))
        .filter(active_member_filter());

    if let Some(eid) = exclude_id {
        query = query.filter(collection_wallpaper::Column::WallpaperId.ne(eid));
    }

    // RANDOM() 是 SQLite 特有函数，通过 Expr::cust 表达
    let record = query
        .order_by(Expr::cust("RANDOM()"), Order::Asc)
        .one(db)
        .await?;
    Ok(record.map(|r| r.wallpaper_id))
}

/// 获取收藏夹中 sort_order 最小的第一张壁纸 ID
async fn first_wallpaper_id(db: &DatabaseConnection, collection_id: i32) -> Result<Option<i32>> {
    let record = collection_wallpaper::Entity::find()
        .filter(collection_wallpaper::Column::CollectionId.eq(collection_id))
        .filter(active_member_filter())
        .order_by_asc(collection_wallpaper::Column::SortOrder)
        .one(db)
        .await?;
    Ok(record.map(|r| r.wallpaper_id))
}

/// 获取收藏夹中 sort_order 最大的最后一张壁纸 ID
async fn last_wallpaper_id(db: &DatabaseConnection, collection_id: i32) -> Result<Option<i32>> {
    let record = collection_wallpaper::Entity::find()
        .filter(collection_wallpaper::Column::CollectionId.eq(collection_id))
        .filter(active_member_filter())
        .order_by_desc(collection_wallpaper::Column::SortOrder)
        .one(db)
        .await?;
    Ok(record.map(|r| r.wallpaper_id))
}

/// 方向枚举，用于 find_adjacent_wallpaper
enum Direction {
    Next,
    Prev,
}

// ==================== 公开的切换壁纸接口 ====================

/// 根据 play_mode 获取收藏夹中的下一张壁纸 ID
///
/// - sequential: 按 sort_order 取当前 wallpaper_id 的下一张，末尾回首
/// - random: RANDOM() 随机取一张（排除当前）
pub async fn next_wallpaper_id(
    db: &DatabaseConnection,
    collection_id: i32,
    current_wallpaper_id: Option<i32>,
    play_mode: &str,
) -> Result<Option<i32>> {
    // 智能收藏夹：下一张由规则命中集求值（sequential 走 id 游标 / random 走 RANDOM()）
    if let Some(model) = collection::Entity::find_by_id(collection_id)
        .one(db)
        .await?
    {
        if model.kind == "smart" {
            return crate::services::smart_collection_service::next_matched_id(
                db,
                &model,
                current_wallpaper_id,
                play_mode,
            )
            .await;
        }
    }

    match play_mode {
        "random" => {
            let result = find_random_wallpaper(db, collection_id, current_wallpaper_id).await?;
            // 只有一张时排除自身后查不到，fallback 取第一张
            if result.is_some() {
                Ok(result)
            } else {
                first_wallpaper_id(db, collection_id).await
            }
        }
        _ => {
            // sequential: 找当前 wallpaper_id 的 sort_order，取下一张
            if let Some(cwid) = current_wallpaper_id {
                if let Some(current_order) = find_sort_order(db, collection_id, cwid).await? {
                    if let Some(wid) =
                        find_adjacent_wallpaper(db, collection_id, current_order, Direction::Next)
                            .await?
                    {
                        return Ok(Some(wid));
                    }
                }
            }
            // fallback: 当前壁纸不在收藏夹中 / 已到末尾 → 回到第一张
            first_wallpaper_id(db, collection_id).await
        }
    }
}

/// 根据 play_mode 获取收藏夹中的上一张壁纸 ID
///
/// - sequential: 按 sort_order 取当前 wallpaper_id 的前一张，首部回末尾
/// - random: 同 next（随机取一张排除当前）
/// - 智能收藏夹: 委托规则命中集求值（sequential 走 id 逆向游标 / random 走 RANDOM()）
/// 相邻壁纸的统一入口：轮播 tick、tray / 快捷键切换共用，
/// 避免两处各自分派 Next / Prev 导致智能收藏夹分流逻辑漂移。
pub async fn adjacent_wallpaper_id(
    db: &DatabaseConnection,
    collection_id: i32,
    current_wallpaper_id: Option<i32>,
    play_mode: &str,
    direction: &SwitchDirection,
) -> Result<Option<i32>> {
    match direction {
        SwitchDirection::Next => {
            next_wallpaper_id(db, collection_id, current_wallpaper_id, play_mode).await
        }
        SwitchDirection::Prev => {
            prev_wallpaper_id(db, collection_id, current_wallpaper_id, play_mode).await
        }
    }
}

pub async fn prev_wallpaper_id(
    db: &DatabaseConnection,
    collection_id: i32,
    current_wallpaper_id: Option<i32>,
    play_mode: &str,
) -> Result<Option<i32>> {
    // 智能收藏夹在 collection_wallpaper 表中无记录，必须先分流，否则 sort_order 查询恒为 None
    if let Some(model) = collection::Entity::find_by_id(collection_id)
        .one(db)
        .await?
    {
        if model.kind == "smart" {
            return crate::services::smart_collection_service::prev_matched_id(
                db,
                &model,
                current_wallpaper_id,
                play_mode,
            )
            .await;
        }
    }

    match play_mode {
        "random" => {
            // 随机模式下上一张等同于随机取一张
            next_wallpaper_id(db, collection_id, current_wallpaper_id, play_mode).await
        }
        _ => {
            // sequential: 找当前 wallpaper_id 的 sort_order，取前一张
            if let Some(cwid) = current_wallpaper_id {
                if let Some(current_order) = find_sort_order(db, collection_id, cwid).await? {
                    if let Some(wid) =
                        find_adjacent_wallpaper(db, collection_id, current_order, Direction::Prev)
                            .await?
                    {
                        return Ok(Some(wid));
                    }
                }
            }
            // fallback: 当前壁纸不在收藏夹中 / 已到首部 → 回到最后一张
            last_wallpaper_id(db, collection_id).await
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use sea_orm::{
        ActiveModelTrait, ColumnTrait, Database, DatabaseConnection, EntityTrait, QueryFilter, Set,
    };
    use sea_orm_migration::MigratorTrait;

    use super::{add_wallpapers, reorder_collections};
    use crate::entities::{collection, collection_wallpaper, wallpaper};
    use crate::migration::Migrator;
    use crate::services::wallpaper_service;

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "mini-wallpaper-add-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    async fn test_db(root: &TempDir) -> DatabaseConnection {
        let path = root.0.join("app.db");
        let db = Database::connect(format!("sqlite:{}?mode=rwc", path.display()))
            .await
            .unwrap();
        Migrator::up(&db, None).await.unwrap();
        db
    }

    async fn insert_wallpaper(db: &DatabaseConnection, name: &str) -> wallpaper::Model {
        wallpaper::ActiveModel {
            name: Set(name.to_string()),
            r#type: Set("image".to_string()),
            file_path: Set(format!("wallpapers/{name}.png")),
            play_count: Set(0),
            created_at: Set("2026-01-01 00:00:00".to_string()),
            updated_at: Set("2026-01-01 00:00:00".to_string()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn insert_collection(db: &DatabaseConnection, kind: &str) -> collection::Model {
        collection::ActiveModel {
            name: Set(kind.to_string()),
            sort_order: Set(0),
            created_at: Set("2026-01-01 00:00:00".to_string()),
            updated_at: Set("2026-01-01 00:00:00".to_string()),
            is_builtin: Set(0),
            kind: Set(kind.to_string()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn membership_count(db: &DatabaseConnection, collection_id: i32) -> u64 {
        collection_wallpaper::Entity::find()
            .filter(collection_wallpaper::Column::CollectionId.eq(collection_id))
            .all(db)
            .await
            .unwrap()
            .len() as u64
    }

    #[tokio::test]
    async fn adds_an_active_wallpaper_to_a_manual_collection() {
        let root = TempDir::new();
        let db = test_db(&root).await;
        let album = insert_collection(&db, "manual").await;
        let image = insert_wallpaper(&db, "ok").await;

        let added = add_wallpapers(&db, album.id, vec![image.id]).await.unwrap();

        assert_eq!(added, 1);
        assert_eq!(membership_count(&db, album.id).await, 1);
        let added_again = add_wallpapers(&db, album.id, vec![image.id]).await.unwrap();
        assert_eq!(added_again, 0);
        assert_eq!(membership_count(&db, album.id).await, 1);
    }

    #[tokio::test]
    async fn rejects_smart_missing_and_trashed_targets() {
        let root = TempDir::new();
        let db = test_db(&root).await;
        let smart = insert_collection(&db, "smart").await;
        let manual = insert_collection(&db, "manual").await;
        let image = insert_wallpaper(&db, "gone").await;
        wallpaper_service::trash_batch(&db, vec![image.id])
            .await
            .unwrap();

        let smart_error = add_wallpapers(&db, smart.id, vec![image.id])
            .await
            .unwrap_err();
        assert!(smart_error.to_string().contains("manual"));
        assert_eq!(membership_count(&db, smart.id).await, 0);

        let missing_collection = add_wallpapers(&db, 999_999, vec![image.id])
            .await
            .unwrap_err();
        assert!(missing_collection
            .to_string()
            .contains("Collection not found"));

        let trashed = add_wallpapers(&db, manual.id, vec![image.id])
            .await
            .unwrap_err();
        assert!(trashed.to_string().contains("not available"));
        assert_eq!(membership_count(&db, manual.id).await, 0);

        let missing_wallpaper = add_wallpapers(&db, manual.id, vec![999_999])
            .await
            .unwrap_err();
        assert!(missing_wallpaper.to_string().contains("not available"));
    }

    #[tokio::test]
    async fn reorders_collections_and_rejects_duplicates() {
        let root = TempDir::new();
        let db = test_db(&root).await;
        let a = insert_collection(&db, "a").await;
        let b = insert_collection(&db, "b").await;

        // 正常重排 [b, a] → b=1, a=2
        reorder_collections(&db, vec![b.id, a.id]).await.unwrap();
        let orders: std::collections::HashMap<i32, i32> = collection::Entity::find()
            .all(&db)
            .await
            .unwrap()
            .into_iter()
            .map(|m| (m.id, m.sort_order))
            .collect();
        assert_eq!(orders[&b.id], 1);
        assert_eq!(orders[&a.id], 2);

        // ActiveModel::update 只应写 sort_order + updated_at，其余 NotSet 字段保持原值；
        // before_save 应刷新 updated_at（哨兵 "2026-01-01" 应被替换）
        let b_after = collection::Entity::find_by_id(b.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(b_after.name, "b");
        assert_eq!(b_after.kind, "b");
        assert_ne!(b_after.updated_at, "2026-01-01 00:00:00");

        // 重复 id：HashSet 去重后集合相等，但长度校验必须拒绝
        assert!(reorder_collections(&db, vec![b.id, a.id, a.id])
            .await
            .is_err());
        // 漏传：集合不等，拒绝
        assert!(reorder_collections(&db, vec![a.id]).await.is_err());
    }
}
