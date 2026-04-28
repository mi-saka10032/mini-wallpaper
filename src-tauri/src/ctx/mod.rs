//! 应用上下文模块
//!
//! `ctx` 目录聚合了所有与应用生命周期一致的全局基础设施组件（纯状态容器）：
//!
//! - **`AppContext`**：全局状态的唯一聚合入口
//! - **`db`**：数据库连接初始化与迁移
//! - **`WallpaperWindowManager`**：壁纸窗口的创建/销毁/事件管理
//!
//! 运行时调度（轮播定时器、全屏检测等后台任务）已迁移至 `runtime/` 模块，
//! `Scheduler` 作为独立的全局 state 与 `AppContext` 平级注册。
//!
//! ## 设计原则
//! - **单一入口**：一个 `AppContext` 贯穿全局，替代多个独立 managed state
//! - **控制反转**：所有子组件在 `new` 时注入 `AppHandle`，方法签名不再传递句柄
//! - **职责分离**：`ctx` 管"是什么"（状态），`runtime` 管"做什么"（调度）

mod db;
pub mod window_manager;

use std::sync::Arc;

use anyhow::Result;
use sea_orm::DatabaseConnection;
use tokio::sync::Mutex;

use window_manager::WallpaperWindowManager;

/// 应用全局上下文
///
/// 聚合所有需要跨模块共享的全局状态，通过 `Arc` 实现零成本克隆。
/// command 层通过 `State<'_, AppContext>` 注入。
///
/// 所有子组件在构造时注入 `AppHandle`，方法调用不再需要外部传递句柄。
pub struct AppContext {
    /// Tauri 应用句柄（用于 emit 事件、获取路径等）
    pub app_handle: tauri::AppHandle,

    /// SeaORM 数据库连接
    pub db: DatabaseConnection,

    /// 壁纸窗口管理器（物理显示器 ↔ 壁纸窗口的映射管理）
    pub window_manager: Arc<Mutex<WallpaperWindowManager>>,
}

impl AppContext {
    /// 构造 AppContext
    ///
    /// 内部通过 `db` 模块完成数据库初始化（连接 + 迁移），
    /// 并将 `app_handle` clone 注入到各子组件，
    /// 实现子组件自持句柄、方法签名零句柄传递。
    pub async fn new(app_handle: tauri::AppHandle) -> Result<Self> {
        let db = db::init_db(&app_handle).await?;
        let window_manager = Arc::new(Mutex::new(WallpaperWindowManager::new(app_handle.clone())));

        Ok(Self {
            window_manager,
            app_handle,
            db,
        })
    }
}
