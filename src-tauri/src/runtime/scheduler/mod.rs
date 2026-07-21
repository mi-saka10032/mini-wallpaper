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
use std::sync::Arc;
use std::collections::HashSet;

use sea_orm::DatabaseConnection;
use tauri::Manager;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::ctx::AppContext;
use crate::ctx::window_manager::WallpaperWindowManager;

// re-export TaskSpawner，保持外部引用路径 `crate::runtime::scheduler::TaskSpawner` 兼容
pub use super::tasks::TaskSpawner;

/// 运行时任务调度器
///
/// 持有 `AppHandle`，通过控制反转向 `TaskSpawner` 注入句柄，
/// 同时承载定时器编排、设置副作用和删除联动等逻辑。
///
/// 作为独立的全局 state 注册到 Tauri，与 `AppContext` 平级，
/// 在 `ExitRequested` 时统一停止所有后台任务。
pub struct Scheduler {
    /// Tauri 应用句柄（注入给 TaskSpawner）
    pub(super) app: tauri::AppHandle,
    /// key -> JoinHandle 映射
    pub(super) tasks: HashMap<String, JoinHandle<()>>,
    /// 已暂停轮播的显示器集合（monitor_id），仅运行时态，不持久化
    ///
    /// 当 `monitor_id` 在该集合内时：
    /// - 轮播定时器被显式 stop
    /// - `start_all_carousel_timers` / `manage_carousel_timer` 跳过该 monitor
    /// - 用户再次触发 TogglePause 时移除 + 通过常规编排恢复
    pub(super) paused_monitors: HashSet<String>,
}

impl Scheduler {
    /// 构造 Scheduler，注入 AppHandle
    pub fn new(app: tauri::AppHandle) -> Self {
        Self {
            app,
            tasks: HashMap::new(),
            paused_monitors: HashSet::new(),
        }
    }

    /// 从 AppHandle 获取 db 连接（clone 后脱离 self 生命周期）
    pub(super) fn db(&self) -> DatabaseConnection {
        self.app.state::<AppContext>().db.clone()
    }

    /// 从 AppHandle 获取 window_manager（clone Arc 后脱离 self 生命周期）
    pub(super) fn window_manager(&self) -> Arc<Mutex<WallpaperWindowManager>> {
        self.app.state::<AppContext>().window_manager.clone()
    }
}