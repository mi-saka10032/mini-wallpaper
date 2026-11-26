//! 应用用例层：编排「写 DB + 渲染副作用 + 任务生命周期」的业务动作。
//!
//! 这一层的存在是为了让前端 command、托盘、全局快捷键、后台任务共享同一份编排：
//! 过去同一个业务动作在多个入口各拼装一遍「预查 → 写库 → 联动 → reconcile」，
//! 容易漏调用或顺序不一致。现在所有跨层写操作都收敛成一个用例函数。
//!
//! ## 依赖方向（单向，不可反转）
//!
//! `application` → `services`（纯 DB）/ `ctx`（纯状态）/ `runtime`（任务生命周期）。
//! `services` 不得反向依赖本层，也不得持有 AppContext / Scheduler，
//! 以保证 68 个 SQLite 单测只需一个临时数据库即可运行。
//!
//! ## 锁序（全仓必须一致）
//!
//! 1. 先做 DB 读写（不持任何应用层锁）
//! 2. 再取 `scheduler` 的 Mutex
//! 3. 最后才可取 `unified_manager` 的 Mutex（reconcile）
//!
//! 反过来（先 manager 再 scheduler）会与删除联动、设置变更联动形成死锁。
//! 本层用例不得在持有 `unified_manager` 锁时再去取 `scheduler` 锁。

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::ctx::AppContext;
use crate::runtime::Scheduler;

pub mod actions;
pub mod collection;
pub mod monitor;
pub mod settings;
pub mod wallpaper;

/// 用例执行所需的显式依赖。
///
/// 由 command 的 `State` 或平台层入口构造后传入，用例内部不做
/// `app.state::<AppContext>()` 反查——那是旧架构里 ctx 散落的根因。
pub struct AppServices<'a> {
    /// 数据库连接与统一壁纸管理器（含 AppHandle，用于 emit / 路径）。
    pub ctx: &'a AppContext,
    /// 后台任务注册表；用例只在需要启停定时器时短暂持锁。
    pub scheduler: &'a Arc<Mutex<Scheduler>>,
}

impl<'a> AppServices<'a> {
    pub fn new(ctx: &'a AppContext, scheduler: &'a Arc<Mutex<Scheduler>>) -> Self {
        Self { ctx, scheduler }
    }
}
