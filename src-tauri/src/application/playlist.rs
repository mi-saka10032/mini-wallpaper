//! 「选下一张 / 上一张」的统一用例。
//!
//! 轮播 tick、托盘菜单、全局快捷键、删除联动都要选图。过去共用
//! `collection_service::adjacent_wallpaper_id` 这个纯 DB 查询；随机播放升级为
//! 环形牌堆后，选图需要读写运行时状态，因此在这里按 `play_mode` 单点分流，
//! 避免两个调用方各自判断导致牌堆状态漂移。
//!
//! 顺序模式仍走原来的游标查询（行为不变）；随机模式才求值命中集并消费牌堆。

use anyhow::Result;
use sea_orm::DatabaseConnection;

use crate::context::AppContext;
use crate::dto::shortcut_dto::Direction;
use crate::entities::monitor_config;
use crate::services::{collection_service, monitor_config_service};
use crate::tasks::carousel::carousel_key;

/// 按当前显示器配置选出相邻壁纸 id。
///
/// - `sequential`：`sort_order` / 智能夹 id 游标，与升级前完全一致。
/// - `random`：环形洗牌牌堆；命中集求值在锁外完成，临界区零 DB await。
///
/// 返回 `None` 表示无可用壁纸（未绑定收藏夹或命中集为空），调用方沿用
/// 既有行为（轮播不计失败、手动切换不写库）。
pub async fn pick_adjacent(
    ctx: &AppContext,
    db: &DatabaseConnection,
    config: &monitor_config::Model,
    direction: &Direction,
) -> Result<Option<i32>> {
    let Some(collection_id) = config.collection_id else {
        return Ok(None);
    };

    if config.play_mode != "random" {
        return collection_service::adjacent_wallpaper_id(
            db,
            collection_id,
            config.wallpaper_id,
            &config.play_mode,
            direction,
        )
        .await;
    }

    // 命中集每次重新求值：任何导入 / 删除 / 恢复 / 规则编辑 / 标签变更
    // 都会让快照与当前集合不同，牌堆据此自动重洗，无需在写路径埋钩子。
    let hit_ids = collection_service::playable_wallpaper_ids(db, collection_id).await?;

    // 同步组只启基准屏一个轮播 timer，但托盘 / 快捷键可能传入组内任意一屏的
    // config。牌堆 key 必须统一到基准屏，否则会生成两条独立序列，
    // 「上一张」与轮播看到的历史对不上。active_targets 返回的 id 已排序，
    // 首个即 sync_group_primary 的口径。
    // 这次查询只用来选 key，失败不应让轮播停摆：降级为按请求屏独立建队列。
    let (targets, is_sync) = monitor_config_service::active_targets(db, &config.monitor_id)
        .await
        .unwrap_or((vec![config.monitor_id.clone()], false));
    let key_mid = if is_sync {
        targets
            .first()
            .cloned()
            .unwrap_or_else(|| config.monitor_id.clone())
    } else {
        config.monitor_id.clone()
    };
    let key = carousel_key(&key_mid);

    // guard 在这条语句结束即释放，后续写库 / reconcile 不持牌堆锁。
    let picked = ctx.play_queues().pick(
        &key,
        collection_id,
        &hit_ids,
        config.wallpaper_id,
        direction,
    );

    Ok(picked)
}
