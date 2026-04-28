//! 运行时任务调度器
//!
//! 全局唯一的异步任务生命周期管理者，所有需要后台执行的任务
//! 均通过此调度器注册 / 停止 / 重启。
//!
//! ## 设计
//! - **`TaskSpawner`**：任务工厂 trait，实现者自身持有所需的全部资源（如 `AppHandle`），
//!   `spawn(self)` 零参数消费自身即可启动任务
//! - **`Scheduler`**：纯粹的 `JoinHandle` 注册表，不持有任何业务资源，
//!   仅负责任务的注册 / 停止 / 重启等生命周期管理

use std::collections::HashMap;

use log::info;
use tokio::task::JoinHandle;

/// 任务工厂 trait
///
/// 业务模块（carousel、fullscreen_detector 等）实现此 trait，
/// struct 自身持有运行所需的全部资源（`AppHandle` + 纯业务参数），
/// `spawn` 消费 self 即可启动异步任务，无需外部注入任何依赖。
///
/// # 示例
/// ```ignore
/// struct MyTask {
///     app: tauri::AppHandle,
///     monitor_id: String,
/// }
///
/// impl TaskSpawner for MyTask {
///     fn spawn(self) -> JoinHandle<()> {
///         tokio::spawn(async move {
///             let ctx = self.app.state::<AppContext>();
///             // 使用 self.monitor_id（业务参数）
///             // 使用 ctx.db / ctx.window_manager（按需获取）
///         })
///     }
/// }
///
/// // 调用方：构造时传入 app_handle，Scheduler 完全不参与资源传递
/// scheduler.spawn("my_task".into(), MyTask { app: handle, monitor_id: "xxx".into() });
/// ```
pub trait TaskSpawner {
    /// 消费 self，创建异步任务并返回 JoinHandle
    ///
    /// 实现者自身已持有所需的全部资源，无需外部参数。
    fn spawn(self) -> JoinHandle<()>;
}

/// 运行时任务调度器
///
/// 纯粹的 `JoinHandle` 注册表，不持有任何业务资源（如 `AppHandle`），
/// 仅负责任务的注册 / 停止 / 重启等生命周期管理。
///
/// 作为独立的全局 state 注册到 Tauri，与 `AppContext` 平级，
/// 在 `ExitRequested` 时统一停止所有后台任务。
pub struct Scheduler {
    /// key -> JoinHandle 映射
    tasks: HashMap<String, JoinHandle<()>>,
}

impl Scheduler {
    /// 构造空的 Scheduler
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
        }
    }

    /// 通过 TaskSpawner trait 创建并注册任务
    ///
    /// 如果同 key 已存在运行中的任务，会先 abort 旧任务再注册新任务。
    pub fn spawn(&mut self, key: String, task: impl TaskSpawner) {
        let handle = task.spawn();
        self.register(key, handle);
    }

    /// 注册一个已有的 JoinHandle（低层 API，优先使用 `spawn`）
    ///
    /// 如果同 key 已存在运行中的任务，会先 abort 旧任务再注册新任务。
    pub fn register(&mut self, key: String, handle: JoinHandle<()>) {
        if let Some(old) = self.tasks.remove(&key) {
            old.abort();
            info!("[Scheduler] 已停止旧任务: {}", key);
        }
        self.tasks.insert(key.clone(), handle);
        info!("[Scheduler] 已注册任务: {}", key);
    }

    /// 停止指定 key 的任务
    pub fn stop(&mut self, key: &str) {
        if let Some(handle) = self.tasks.remove(key) {
            handle.abort();
            info!("[Scheduler] 已停止任务: {}", key);
        }
    }

    /// 停止所有任务
    pub fn stop_all(&mut self) {
        let count = self.tasks.len();
        for (key, handle) in self.tasks.drain() {
            handle.abort();
            info!("[Scheduler] 已停止任务: {}", key);
        }
        info!("[Scheduler] 已停止全部 {} 个任务", count);
    }

    /// 检查指定 key 是否有运行中的任务
    pub fn is_running(&self, key: &str) -> bool {
        self.tasks
            .get(key)
            .map(|h| !h.is_finished())
            .unwrap_or(false)
    }

    /// 通过 TaskSpawner 重启任务
    ///
    /// 语义上等同于 `spawn`（内部已处理旧任务），更明确地表达"重启"意图。
    pub fn restart(&mut self, key: String, task: impl TaskSpawner) {
        self.spawn(key, task);
    }
}
