//! 壁纸相关用例：删除 / 彻底删除。
//!
//! 两个 command 过去各自重复「预查受影响 config → 写库 → 删除联动」三步，
//! 差异仅在软删除还是硬删除，因此合并为一个用例 + 模式参数。

use std::collections::HashSet;

use anyhow::Result;

use super::AppServices;
use crate::services::{monitor_config_service, wallpaper_service};

/// 删除方式：影响调用哪个数据层写入，联动逻辑完全相同。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeletionMode {
    /// 移入回收站（软删除）
    Trash,
    /// 彻底删除
    Purge,
}

/// 删除壁纸并联动清理桌面显示、轮播定时器与窗口。
///
/// 顺序是硬约束：必须在写库**之前**取受影响 config 的快照，
/// 因为写库会把 `monitor_configs.wallpaper_id` 置空；
/// 联动必须在写库之后，且只持一次 scheduler 锁。
pub async fn delete_wallpapers(
    svc: &AppServices<'_>,
    ids: Vec<i32>,
    mode: DeletionMode,
) -> Result<u64> {
    let deleted_ids: HashSet<i32> = ids.iter().copied().collect();

    // 1. 删除前快照：写库后 wallpaper_id 被置空，事后查不到引用关系
    let affected_configs =
        monitor_config_service::get_configs_by_wallpaper_ids(&svc.ctx.db, &ids).await?;

    // 2. 数据层写入
    let deleted = match mode {
        DeletionMode::Trash => wallpaper_service::trash_batch(&svc.ctx.db, ids).await?,
        DeletionMode::Purge => wallpaper_service::delete_batch(&svc.ctx.db, ids).await?,
    };

    // 3. 联动（定时器 + 窗口 + reconcile）：由 Scheduler 内部的 effects 完成，
    //    阶段 2 会把这段编排也搬进本层，此处先保持行为完全一致。
    let mut sched = svc.scheduler.lock().await;
    sched
        .on_wallpapers_deleted(&affected_configs, &deleted_ids)
        .await;

    Ok(deleted)
}
