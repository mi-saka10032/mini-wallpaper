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

use std::collections::HashMap;

use sea_orm::DatabaseConnection;
use tauri::Manager;
use tokio::task::JoinHandle;

use crate::ctx::AppContext;

// re-export TaskSpawner，保持外部引用路径 `crate::runtime::scheduler::TaskSpawner` 兼容
pub use super::tasks::TaskSpawner;

/// 运行时任务调度器
///
/// 持有 `AppHandle`，通过控制反转向 `TaskSpawner` 注入句柄，
/// 同时承载定时器编排、设置副作用和删除联动等逻辑。
///
/// 作为独立的全局 state 注册到 Tauri，与 `AppContext` 平级。
/// 进程退出后任务随运行时一起结束。
pub struct Scheduler {
    /// Tauri 应用句柄（注入给 TaskSpawner）
    pub(super) app: tauri::AppHandle,
    /// key -> JoinHandle 映射，key 是稳定标识（如 `carousel:<monitor_id>` / `fullscreen_detector`）。
    ///
    /// 这张表本身就是「任务是否在跑」的唯一事实来源：同 key 再 spawn 会先 abort 旧任务，
    /// 因此注册/停止/重启都按 key 幂等处理，不需要额外的 running 标志位。
    pub(super) tasks: HashMap<String, JoinHandle<()>>,
}

impl Scheduler {
    /// 构造 Scheduler，注入 AppHandle
    pub fn new(app: tauri::AppHandle) -> Self {
        Self {
            app,
            tasks: HashMap::new(),
        }
    }

    /// 从 AppHandle 获取 db 连接（clone 后脱离 self 生命周期）
    pub(super) fn db(&self) -> DatabaseConnection {
        let app_ctx: &AppContext = self.app.state::<AppContext>().inner();
        app_ctx.db.clone()
    }

    /// 取回全局 AppContext（Tauri manage 注册的唯一实例）。
    ///
    /// 简化后 AppContext 内不再套 Arc，各处按需经 AppHandle 取同一实例即可，
    /// 无需 clone 字段所有权来脱离 self 生命周期。
    pub(super) fn app_context(&self) -> tauri::State<'_, AppContext> {
        self.app.state::<AppContext>()
    }
}
