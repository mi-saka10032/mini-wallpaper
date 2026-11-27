//! 用户动作用例：前端 trigger_action、托盘、全局快捷键共用同一入口。

use log::warn;

use super::{effects::AppEffects, scene, AppServices};
use crate::dto::action::Action;

pub async fn dispatch_action(svc: &AppServices<'_>, action: Action) {
    // 场景应用必须在锁外拦截：apply_scene 内部会取 scheduler 锁走副作用编排，
    // 若先在这里持有 guard 就会自锁。用 &action 借用，后续仍能把所有权交给 fx。
    match &action {
        Action::ApplyScene { scene_id } => {
            if let Err(e) = scene::apply_scene(svc, *scene_id).await {
                warn!("[Action] 应用场景 {} 失败: {:#}", scene_id, e);
            }
            return;
        }
        _ => {}
    }

    let mut guard = svc.scheduler.lock().await;
    let mut fx = AppEffects::new(svc.ctx, &mut guard);
    fx.dispatch_action(action).await;
}
