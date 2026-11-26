//! 副作用编排的执行上下文。
//!
//! 原来这些编排挂在 `impl TaskRegistry` 上，靠 `app.state::<Arc<AppContext>>()` 反查资源，
//! 导致 ctx / scheduler 散落在业务代码里。现在统一由 `AppEffects` 显式持有
//! `&AppContext` 与 `&mut TaskRegistry`，业务层零反查。
//!
//! 保留 `db()` / `app_context()` / `stop()` / `restart()` / `is_running()` / `spawn()`
//! 等同名薄方法，让搬迁过来的编排保持原有调用形态，把迁移风险降到最低。

use sea_orm::DatabaseConnection;

use crate::context::AppContext;
use crate::tasks::registry::TaskRegistry;
use crate::tasks::TaskSpawner;

pub mod action_dispatch;
pub mod deletion;
pub mod favorite;
pub mod setting;
pub mod timers;

pub struct AppEffects<'a> {
    pub ctx: &'a AppContext,
    pub registry: &'a mut TaskRegistry,
    /// AppHandle 的 clone：窗口 / emit / 退出等操作仍以 `self.app` 形式使用，
    /// 避免逐处改写编排代码。来源是 AppContext，不是新增的全局状态。
    pub app: tauri::AppHandle,
}

impl<'a> AppEffects<'a> {
    pub fn new(ctx: &'a AppContext, registry: &'a mut TaskRegistry) -> Self {
        Self {
            ctx,
            registry,
            app: ctx.app_handle.clone(),
        }
    }

    /// 编排代码需要一个脱离借用生命周期的 DB 句柄时用它（clone 很轻）。
    pub fn db(&self) -> DatabaseConnection {
        self.ctx.db.clone()
    }

    pub fn app_context(&self) -> &AppContext {
        self.ctx
    }

    pub fn stop(&mut self, key: &str) {
        self.registry.stop(key);
    }

    pub fn is_running(&self, key: &str) -> bool {
        self.registry.is_running(key)
    }

    pub fn restart(&mut self, key: String, task: impl TaskSpawner) {
        self.registry.restart(key, task);
    }

    pub fn spawn(&mut self, key: String, task: impl TaskSpawner) {
        self.registry.spawn(key, task);
    }
}
