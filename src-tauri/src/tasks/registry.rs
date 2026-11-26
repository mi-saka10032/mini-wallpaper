//! 后台任务注册表。
//!
//! 原 `Scheduler` 承载了大量业务编排（设置变更、删除联动、动作派发），那些逻辑
//! 已迁往 `application/effects`。这里只保留 `key -> JoinHandle` 的纯生命周期管理：
//! 同 key 再 spawn 会先 abort 旧任务，因此所有操作按 key 幂等。

use std::collections::HashMap;

use log::info;
use sea_orm::DatabaseConnection;
use tauri::Manager;
use tokio::task::JoinHandle;

use crate::context::AppContext;
use crate::tasks::TaskSpawner;

pub struct TaskRegistry {
    /// 注入给 TaskSpawner；阶段 3 收尾时会改为显式 TaskDeps。
    pub(crate) app: tauri::AppHandle,
    pub(crate) tasks: HashMap<String, JoinHandle<()>>,
}

impl TaskRegistry {
    // 过渡方法：effects 迁往 AppEffects 后删除，届时注册表不再反查 AppContext。
    pub(crate) fn db(&self) -> DatabaseConnection {
        let ctx: &AppContext = self.app.state::<AppContext>().inner();
        ctx.db.clone()
    }

    pub(crate) fn app_context(&self) -> tauri::State<'_, AppContext> {
        self.app.state::<AppContext>()
    }

    pub fn new(app: tauri::AppHandle) -> Self {
        Self {
            app,
            tasks: HashMap::new(),
        }
    }

    /// 创建并注册任务；同 key 已有任务会先 abort。
    pub fn spawn(&mut self, key: String, task: impl TaskSpawner) {
        let handle = task.spawn(&self.app);
        self.register(key, handle);
    }

    pub fn register(&mut self, key: String, handle: JoinHandle<()>) {
        if let Some(old) = self.tasks.remove(&key) {
            old.abort();
            info!("[TaskRegistry] 已停止旧任务: {}", key);
        }
        self.tasks.insert(key.clone(), handle);
        info!("[TaskRegistry] 已注册任务: {}", key);
    }

    pub fn stop(&mut self, key: &str) {
        if let Some(handle) = self.tasks.remove(key) {
            handle.abort();
            info!("[TaskRegistry] 已停止任务: {}", key);
        }
    }

    pub fn stop_all(&mut self) {
        let count = self.tasks.len();
        for (key, handle) in self.tasks.drain() {
            handle.abort();
            info!("[TaskRegistry] 已停止任务: {}", key);
        }
        info!("[TaskRegistry] 已停止全部 {} 个任务", count);
    }

    pub fn is_running(&self, key: &str) -> bool {
        self.tasks
            .get(key)
            .map(|h| !h.is_finished())
            .unwrap_or(false)
    }

    /// 语义上等同于 spawn，明确表达「重启」意图。
    pub fn restart(&mut self, key: String, task: impl TaskSpawner) {
        self.spawn(key, task);
    }
}
