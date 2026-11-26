//! 用户动作派发 Command
//!
//! 暴露统一的 [`trigger_action`] 命令给前端调用。前端只关心"想做什么"，
//! 显示器解析、display_mode 适配、定时器重置等细节内聚在 application 层。
//!
//! 前端、全局快捷键、托盘菜单三方共享 `application::actions::dispatch_action`
//! 单一入口；本命令只是薄壳。

use std::sync::Arc;

use tauri::State;
use tokio::sync::Mutex;

use crate::application::{self, AppServices};
use crate::context::AppContext;
use crate::runtime::action::Action;
use crate::runtime::Scheduler;

use super::error::CommandResult;

/// 派发一个用户动作（Next / Prev / TogglePause / ...）
///
/// 前端约定的载荷形如：`invoke('trigger_action', { action: { type: 'next' } })`
///
/// 该命令吞掉所有非致命错误（仅记录 warn 日志），始终返回 `Ok(())`，
/// 避免快捷键场景下因偶发错误导致用户看到红色报错弹窗。
#[tauri::command]
pub async fn trigger_action(
    ctx: State<'_, AppContext>,
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
    action: Action,
) -> CommandResult<()> {
    let svc = AppServices::new(ctx.inner(), scheduler.inner());
    application::actions::dispatch_action(&svc, action).await;
    Ok(())
}
