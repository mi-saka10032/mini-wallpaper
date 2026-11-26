//! 运行时任务调度器
//!
//! 全局唯一的异步任务生命周期管理者，所有需要后台执行的任务
//! 均通过此调度器注册 / 停止 / 重启。
//!
//! ## 设计
//! - **`TaskSpawner`** trait 定义在 `tasks/mod.rs`，实现者仅持有纯业务参数
//! - **`Scheduler`**：持有 `AppHandle` 的任务调度器，
//!   负责任务的注册 / 停止 / 重启等生命周期管理
//!
//! ## 职责拆分（跨文件 impl）
//! - **`mod.rs`**（本文件）：核心 struct 定义 + 共享资源访问
//! - **`task_lifecycle.rs`**：JoinHandle 生命周期管理 + 轮播/全屏检测编排
//! - **`setting_effects.rs`**：设置变更副作用（音量、全屏检测开关、display_mode）
//! - **`deletion_effects.rs`**：删除联动（壁纸删除、收藏夹删除、收藏夹移除壁纸）
//! - **`favorite_effects.rs`**：收藏切换联动（ToggleFavorite 快捷键 / 托盘 / 红心按钮共用）
//! - **`action_dispatch.rs`**：用户动作派发（Next/Prev/... 快捷键 & 托盘菜单共用）

mod action_dispatch;
mod deletion_effects;
mod favorite_effects;
mod setting_effects;
mod task_lifecycle;

// 过渡别名：effects 仍以 `impl Scheduler` 存在，阶段 3 收尾时删除别名并全仓替换。
pub use crate::tasks::registry::TaskRegistry as Scheduler;
