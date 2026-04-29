//! 运行时调度层
//!
//! 管理所有后台异步任务的生命周期，与 `ctx`（纯状态容器）平级。
//!
//! - **`Scheduler`**：持有 `AppHandle` 的任务调度器，通过控制反转向任务注入句柄，
//!   同时承载定时器编排和设置副作用等联动逻辑
//! - **`TaskSpawner`**：任务工厂 trait，实现者仅持有纯业务参数，
//!   `spawn` 接收调度器注入的 `&AppHandle` 按需获取共享资源
//! - **`carousel`**：轮播定时器（`CarouselTask` 实现 `TaskSpawner`）
//! - **`fullscreen_detector`**：全屏检测（`FullscreenDetectionTask` 实现 `TaskSpawner`）

pub mod carousel;
pub mod fullscreen_detector;
mod scheduler;

pub use scheduler::Scheduler;
