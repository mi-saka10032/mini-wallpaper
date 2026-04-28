//! 运行时调度层
//!
//! 管理所有后台异步任务的生命周期，与 `ctx`（纯状态容器）平级。
//!
//! - **`Scheduler`**：纯 `JoinHandle` 注册表，不持有任何业务资源，
//!   仅负责任务的注册 / 停止 / 重启等生命周期管理
//! - **`TaskSpawner`**：任务工厂 trait，实现者自身持有所需的全部资源，
//!   `spawn(self)` 零参数消费自身即可启动任务
//! - **`carousel`**：轮播定时器（`CarouselTask` 实现 `TaskSpawner`）
//! - **`fullscreen_detector`**：全屏检测（`FullscreenDetectionTask` 实现 `TaskSpawner`）

pub mod carousel;
pub mod fullscreen_detector;
mod scheduler;

pub use scheduler::Scheduler;
