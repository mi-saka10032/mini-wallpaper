//! 智能收藏夹成员求值服务
//!
//! 智能收藏夹（方案 A1）不物化成员：命中集由 `collections.rule_json` 编译成
//! 参数化查询，对 `wallpapers` 表实时求值。本模块提供成员集合、计数、以及
//! 轮播「下一张」的求值原语，供 `collection_service` 在 `kind = smart` 分支委托。
//!
//! ## 顺序约定
//!
//! - sequential：按 `wallpaper.id` 升序（稳定游标，与手动收藏夹的 sort_order 无关）。
//! - random：命中集内随机，排除当前张。
//!
//! ## 与手动收藏夹的对称性
//!
//! `collection_service` 的 `get_wallpapers` / `count_wallpapers` /
//! `next_wallpaper_id` / `has_enough_wallpapers` 会先查 `kind`，smart 分支
//! 落到本模块。因此上层（轮播任务、显示器绑定、定时器编排）无需感知两类差异。

use anyhow::{anyhow, Result};
use sea_orm::prelude::Expr;
use sea_orm::*;

use crate::entities::{collection, wallpaper};
use crate::services::smart_rule::SmartRule;

/// 从收藏夹模型加载并校验规则；非 smart 或规则缺失时报错
fn load_rule(model: &collection::Model) -> Result<SmartRule> {
    if model.kind != "smart" {
        return Err(anyhow!("收藏夹 {} 不是智能收藏夹", model.id));
    }
    let json = model
        .rule_json
        .as_deref()
        .ok_or_else(|| anyhow!("智能收藏夹 {} 缺少规则", model.id))?;
    SmartRule::parse_and_validate(json)
}

/// 求值命中的完整壁纸列表（按 id 升序）
pub async fn matched_wallpapers(
    db: &DatabaseConnection,
    model: &collection::Model,
) -> Result<Vec<wallpaper::Model>> {
    let rule = load_rule(model)?;
    let cond = rule.build_condition()?;
    let rows = wallpaper::Entity::find()
        .filter(cond)
        .order_by_asc(wallpaper::Column::Id)
        .all(db)
        .await?;
    Ok(rows)
}

/// 命中数（参数化 COUNT，不物化 id）
pub async fn count_matched(db: &DatabaseConnection, model: &collection::Model) -> Result<u64> {
    let rule = load_rule(model)?;
    count_matched_by_rule(db, &rule).await
}

/// 按规则直接计数（供保存前预览）
pub async fn count_matched_by_rule(db: &DatabaseConnection, rule: &SmartRule) -> Result<u64> {
    let cond = rule.build_condition()?;
    let count = wallpaper::Entity::find().filter(cond).count(db).await?;
    Ok(count)
}

/// 轮播「下一张」求值
///
/// sequential：命中集升序中取 current 的后继，末尾 wrap 回首张；
/// random：命中集内随机排除 current，仅一张时回退首张。
pub async fn next_matched_id(
    db: &DatabaseConnection,
    model: &collection::Model,
    current: Option<i32>,
    play_mode: &str,
) -> Result<Option<i32>> {
    let rule = load_rule(model)?;
    let base_cond = rule.build_condition()?;

    match play_mode {
        "random" => {
            // 排除当前张后 RANDOM() 取一张；命中集≤1 时回退首张（含当前张）
            let mut cond = base_cond.clone();
            if let Some(c) = current {
                cond = cond.add(wallpaper::Column::Id.ne(c));
            }
            let picked = wallpaper::Entity::find()
                .filter(cond)
                .order_by(Expr::cust("RANDOM()"), Order::Asc)
                .one(db)
                .await?
                .map(|w| w.id);
            if picked.is_some() {
                return Ok(picked);
            }
            // 排除后为空（收藏夹仅一张或为空）→ 回退首张
            let first = wallpaper::Entity::find()
                .filter(base_cond)
                .order_by_asc(wallpaper::Column::Id)
                .one(db)
                .await?
                .map(|w| w.id);
            Ok(first)
        }
        _ => {
            // sequential：取 id 严格大于 current 的最小者；找不到则 wrap 回首张
            if let Some(c) = current {
                let next = wallpaper::Entity::find()
                    .filter(base_cond.clone())
                    .filter(wallpaper::Column::Id.gt(c))
                    .order_by_asc(wallpaper::Column::Id)
                    .one(db)
                    .await?
                    .map(|w| w.id);
                if next.is_some() {
                    return Ok(next);
                }
            }
            // current 为空 / 已到末尾 / current 不在命中集 → 回首张
            let first = wallpaper::Entity::find()
                .filter(base_cond)
                .order_by_asc(wallpaper::Column::Id)
                .one(db)
                .await?
                .map(|w| w.id);
            Ok(first)
        }
    }
}

/// 轮播「上一张」求值
///
/// sequential：命中集升序中取 current 的前驱，首部 wrap 回末张；
/// random：与 next 同义（命中集内随机排除 current），直接委托 `next_matched_id`。
pub async fn prev_matched_id(
    db: &DatabaseConnection,
    model: &collection::Model,
    current: Option<i32>,
    play_mode: &str,
) -> Result<Option<i32>> {
    if play_mode == "random" {
        return next_matched_id(db, model, current, play_mode).await;
    }

    let rule = load_rule(model)?;
    let base_cond = rule.build_condition()?;

    // sequential：取 id 严格小于 current 的最大者；找不到则 wrap 回末张
    if let Some(c) = current {
        let prev = wallpaper::Entity::find()
            .filter(base_cond.clone())
            .filter(wallpaper::Column::Id.lt(c))
            .order_by_desc(wallpaper::Column::Id)
            .one(db)
            .await?
            .map(|w| w.id);
        if prev.is_some() {
            return Ok(prev);
        }
    }
    // current 为空 / 已到首部 / current 不在命中集 → 回末张
    let last = wallpaper::Entity::find()
        .filter(base_cond)
        .order_by_desc(wallpaper::Column::Id)
        .one(db)
        .await?
        .map(|w| w.id);
    Ok(last)
}