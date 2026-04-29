//! 后台任务实现
//!
//! 每个任务实现 `TaskSpawner` trait，仅持有纯业务参数，
//! `spawn` 接收调度器注入的 `&AppHandle`，按需获取共享资源。
//!
//! - **`carousel`**：轮播定时器（`CarouselTask` 实现 `TaskSpawner`）
//! - **`fullscreen_detector`**：全屏检测（`FullscreenDetectionTask` 实现 `TaskSpawner`）

use tokio::task::JoinHandle;

pub mod carousel;
pub mod fullscreen_detector;

/// 任务工厂 trait
///
/// 业务模块（carousel、fullscreen_detector 等）实现此 trait，
/// struct 仅持有纯业务参数（如 `monitor_id`），
/// `spawn` 接收 `&AppHandle` 由调度器注入，按需获取 db / window_manager 等共享资源。
///
/// # 示例
/// ```ignore
/// struct MyTask { monitor_id: String }
///
/// impl TaskSpawner for MyTask {
///     fn spawn(self, app: &tauri::AppHandle) -> JoinHandle<()> {
///         let app = app.clone();
///         tokio::spawn(async move {
///             let ctx = app.state::<AppContext>();
///             // 使用 self.monitor_id + ctx.db / ctx.window_manager
///         })
///     }
/// }
/// ```
pub trait TaskSpawner {
    /// 消费 self，接收调度器注入的 AppHandle，创建异步任务并返回 JoinHandle
    fn spawn(self, app: &tauri::AppHandle) -> JoinHandle<()>;
}