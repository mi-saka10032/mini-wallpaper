//! 运行时调度层
//!
//! 管理所有后台异步任务的生命周期，与 `ctx`（纯状态容器）平级。
//!
//! ## 目录结构
//! - **`scheduler/`**：任务调度器（跨文件 impl 拆分为 4 类职责）
//!   - `mod.rs`：核心 struct 定义 + 共享资源访问
//!   - `task_lifecycle.rs`：JoinHandle 生命周期管理 + 轮播/全屏检测编排
//!   - `setting_effects.rs`：设置变更副作用
//!   - `deletion_effects.rs`：删除联动
//! - **`tasks/`**：后台任务定义（`TaskSpawner` trait + 各任务实现）
//!   - `mod.rs`：`TaskSpawner` trait 定义
//!   - `carousel.rs`：轮播定时器
//!   - `fullscreen_detector.rs`：全屏检测

pub mod scheduler;
pub mod tasks;

pub use scheduler::Scheduler;
