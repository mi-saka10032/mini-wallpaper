//! 显示器配置用例：一次写入整组 + 定时器 +  reconcile 的编排。

use anyhow::{Context, Result};

use super::{effects::AppEffects, AppServices};
use crate::context::unified_manager::ReconcileScope;
use crate::dto::monitor_config_dto::UpsertMonitorConfigRequest;
use crate::entities::monitor_config;
use crate::services::monitor_config_service;
use crate::tasks::carousel::carousel_key;

/// 更新显示器配置（同步组由 service 裁决），并统一下发渲染副作用。
///
/// 锁序固定：先 DB → 取 scheduler 锁管定时器 → drop → 再取 unified_manager 锁 reconcile。
/// 同步组只保留基准屏一个轮播 timer，否则 N 个 timer 会让切换频率翻 N 倍。
pub async fn upsert_monitor_config(
    svc: &AppServices<'_>,
    req: &UpsertMonitorConfigRequest,
) -> Result<monitor_config::Model> {
    let need_restart = req.play_interval.is_some() || req.collection_id.is_some();
    let (updated, is_sync) = monitor_config_service::upsert_group(&svc.ctx.db, req).await?;

    // 切到顺序播放后牌堆不再被消费，全部清空；切回 random 时下一次消费
    // 会以当前显示图为锚重建。用 clear 而非删单 key，是因为同步组的牌堆
    // key 统一在基准屏，请求里的 monitor_id 未必就是基准屏。
    if req.play_mode.as_deref() == Some("sequential") {
        svc.ctx.play_queues().clear();
    }

    {
        let mut guard = svc.scheduler.lock().await;
        let mut fx = AppEffects::new(svc.ctx, &mut guard);
        for config in &updated {
            if is_sync && config.monitor_id != req.monitor_id {
                fx.stop(&carousel_key(&config.monitor_id));
                continue;
            }
            fx.manage_carousel_timer(config, need_restart).await;
        }
    }

    let scope = if is_sync {
        ReconcileScope::All
    } else {
        ReconcileScope::One(req.monitor_id.clone())
    };
    svc.ctx
        .unified_manager
        .lock()
        .await
        .reconcile(scope)
        .await?;

    updated
        .into_iter()
        .find(|c| c.monitor_id == req.monitor_id)
        .context("基准屏配置未返回")
}
