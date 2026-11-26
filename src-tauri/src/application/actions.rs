//! 用户动作用例：前端 trigger_action、托盘、全局快捷键共用同一入口。

use super::{effects::AppEffects, AppServices};
use crate::dto::action::Action;

pub async fn dispatch_action(svc: &AppServices<'_>, action: Action) {
    let mut guard = svc.scheduler.lock().await;
    let mut fx = AppEffects::new(svc.ctx, &mut guard);
    fx.dispatch_action(action).await;
}
