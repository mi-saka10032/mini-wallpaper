//! 通用定时器注册表
//!
//! 全局唯一的定时器生命周期管理者，所有需要后台定时执行的任务
//! 均通过此注册表注册 / 停止 / 重启。
//! 业务模块（全屏检测、轮播切换等）只需提供 `JoinHandle`，
//! 不再自行持有句柄，实现控制反转。

use std::collections::HashMap;
use std::sync::Arc;

use log::info;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

/// 定时器注册表
pub struct TimerRegistry {
    timers: HashMap<String, JoinHandle<()>>,
}

impl TimerRegistry {
    pub fn new() -> Self {
        Self {
            timers: HashMap::new(),
        }
    }

    /// 注册一个定时任务
    ///
    /// 如果同 key 已存在运行中的任务，会先 abort 旧任务再注册新任务。
    pub fn register(&mut self, key: String, handle: JoinHandle<()>) {
        // 如果已有同名任务，先停止
        if let Some(old) = self.timers.remove(&key) {
            old.abort();
            info!("[TimerRegistry] 已停止旧任务: {}", key);
        }
        self.timers.insert(key.clone(), handle);
        info!("[TimerRegistry] 已注册任务: {}", key);
    }

    /// 停止指定 key 的定时任务
    pub fn stop(&mut self, key: &str) {
        if let Some(handle) = self.timers.remove(key) {
            handle.abort();
            info!("[TimerRegistry] 已停止任务: {}", key);
        }
    }

    /// 停止所有定时任务
    pub fn stop_all(&mut self) {
        let count = self.timers.len();
        for (key, handle) in self.timers.drain() {
            handle.abort();
            info!("[TimerRegistry] 已停止任务: {}", key);
        }
        info!("[TimerRegistry] 已停止全部 {} 个任务", count);
    }

    /// 检查指定 key 是否有运行中的任务
    pub fn is_running(&self, key: &str) -> bool {
        self.timers
            .get(key)
            .map(|h| !h.is_finished())
            .unwrap_or(false)
    }

    /// 重启：先停止旧任务，再注册新任务
    ///
    /// 与 `register` 行为一致（register 内部已处理旧任务），
    /// 语义上更明确地表达"重启"意图。
    pub fn restart(&mut self, key: String, handle: JoinHandle<()>) {
        self.register(key, handle);
    }
}

/// TimerRegistry 的 managed state 类型别名
pub type TimerRegistryState = Arc<Mutex<TimerRegistry>>;

/// 创建 TimerRegistry state
pub fn create_timer_registry() -> TimerRegistryState {
    Arc::new(Mutex::new(TimerRegistry::new()))
}
