//! 后台任务实现
//!
//! 每个任务实现 `TaskSpawner` trait，仅持有纯业务参数，
//! `spawn` 接收调度器注入的 `&AppHandle`，按需获取共享资源。
//!
//! - **`carousel`**：轮播定时器（`CarouselTask` 实现 `TaskSpawner`）
//! - **`fullscreen_detector`**：全屏检测（`FullscreenDetectionTask` 实现 `TaskSpawner`）

use std::sync::Arc;

use tokio::task::JoinHandle;

use crate::context::AppContext;

/// 后台任务的显式依赖：注册时注入，任务体内不再反查 AppHandle state。
#[derive(Clone)]
pub struct TaskDeps {
    pub ctx: Arc<AppContext>,
}

pub mod carousel;
pub mod fullscreen_detector;
pub mod registry;

/// 任务工厂 trait
///
/// 业务模块（carousel、fullscreen_detector 等）实现此 trait，
/// struct 仅持有纯业务参数（如 `monitor_id`），
/// `spawn` 接收显式注入的 `TaskDeps`，任务体内直接取 db / unified_manager。
///
/// # 示例
/// ```ignore
/// struct MyTask { monitor_id: String }
///
/// impl TaskSpawner for MyTask {
///     fn spawn(self, app: &tauri::AppHandle) -> JoinHandle<()> {
///         tokio::spawn(async move {
///             // 使用 self.monitor_id + deps.ctx.db / deps.ctx.unified_manager
///         })
///     }
/// }
/// ```
pub trait TaskSpawner {
    /// 消费 self，接收调度器注入的 AppHandle，创建异步任务并返回 JoinHandle
    fn spawn(self, deps: TaskDeps) -> JoinHandle<()>;
}
