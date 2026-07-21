//! 用户动作（Action）类型定义
//!
//! 把"用户希望发生什么"抽象为强类型枚举，由 `Scheduler::dispatch_action`
//! 统一解释执行。三类调用方共享同一份 Action 定义：
//!
//! - **前端**：通过 `trigger_action` Tauri 命令派发（serde 反序列化）
//! - **全局快捷键**（Phase 2）：键盘事件 → 映射为 Action → dispatch
//! - **托盘菜单**（Phase 2）：菜单项 click → 映射为 Action → dispatch
//!
//! ## 设计原则
//!
//! - **纯数据**：本文件只定义 Action 枚举本身，不含任何执行逻辑，
//!   执行交由 `runtime/scheduler/action_dispatch.rs`
//! - **序列化兼容**：使用 `#[serde(tag = "type", rename_all = "camelCase")]`，
//!   前端约定的载荷形如 `{ "type": "next" }` / `{ "type": "togglePause" }`，
//!   未来含载荷的动作可平滑扩展为 `{ "type": "setWallpaper", "wallpaperId": 42 }`
//! - **目标作用域内聚**：动作的目标显示器解析在 dispatcher 内部完成，
//!   调用方无需感知 display_mode 与 active 显示器探测细节

use serde::Deserialize;

/// 用户动作枚举
///
/// 当前阶段（Phase 1）落地：Next / Prev / TogglePause / OpenMain / Quit。
///
/// `ToggleFavorite`：收藏 / 取消收藏"当前显示中的壁纸"。收藏目标 id 的权威源
/// 是 `monitor_configs.wallpaper_id`——轮播 tick 与手动切换都会实时回写，
/// 因此后端可直接从 active config 读取当前壁纸 id，无需前端透传。
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Action {
    /// 切换到下一张壁纸
    Next,
    /// 切换到上一张壁纸
    Prev,
    /// 暂停 / 恢复轮播（针对 active 目标）
    TogglePause,
    /// 显示并聚焦主窗口
    OpenMain,
    /// 退出应用
    Quit,
    /// 收藏 / 取消收藏当前显示中的壁纸（切换其在内置「我喜欢」收藏夹中的归属）
    ToggleFavorite,
}