//! 用户动作派发 Command
//!
//! 暴露统一的 [`trigger_action`] 命令给前端调用。前端只关心"想做什么"，
//! 显示器解析、display_mode 适配、定时器重置等细节全部内聚在
//! `Scheduler::dispatch_action` 中。
//!
//! 该命令也是后续全局快捷键 / 托盘菜单 → Action 路由路径的**参考实现**：
//! 三方调用方共享 `Scheduler::dispatch_action` 单一入口。

use std::sync::Arc;

use tauri::State;
use tokio::sync::Mutex;

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
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
    action: Action,
) -> CommandResult<()> {
    let mut sched = scheduler.lock().await;
    sched.dispatch_action(action).await;
    Ok(())
}
