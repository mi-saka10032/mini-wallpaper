//! 用户动作用例：前端 trigger_action、托盘、全局快捷键共用同一入口。

use super::AppServices;
use crate::runtime::action::Action;

pub async fn dispatch_action(svc: &AppServices<'_>, action: Action) {
    let mut sched = svc.scheduler.lock().await;
    sched.dispatch_action(action).await;
}
