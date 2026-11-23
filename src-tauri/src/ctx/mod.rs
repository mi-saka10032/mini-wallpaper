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
/// 配置快照与 diff 引擎（架构重整 Phase 1，纯逻辑无 IO，供后续 UnifiedWallpaperManager 消费）
pub mod reconcile;
pub mod toast_manager;
/// 统一壁纸管理器：主体 + native（渲染链路）/ webview（窗口链路）两个 impl 文件
pub mod unified_manager;

use std::sync::Arc;

use anyhow::Result;
use sea_orm::DatabaseConnection;
use tokio::sync::Mutex;

use toast_manager::ToastManager;
use unified_manager::{RenderCoordinator, UnifiedWallpaperManager, WallpaperWindowManager};

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
    ///
    /// 用 `Arc` 是因为它不仅被 AppContext 持有，还要交给协调器与 Scheduler 共享；
    /// `Mutex` 保证窗口的创建/销毁与通知不会并发交错。
    pub window_manager: Arc<Mutex<WallpaperWindowManager>>,

    /// 渲染协调器（壁纸渲染链路唯一入口，决定 Native / WebView backend）
    ///
    /// 内部持有 per-monitor 的可变状态（backends / wallpaper_ids / 几何），
    /// 因此必须整体加锁：调用方只锁这一个 Mutex，避免与 window_manager 形成双锁持锁。
    pub render_coordinator: Arc<Mutex<RenderCoordinator>>,

    /// 统一壁纸管理器（架构重整 Phase 2）：monitor_id 维度的统一入口，
    /// 启动时完成显示器枚举与几何登记，后续 action 的 reconcile 派发点。
    // Phase 2 只在 new() 里构造并 init；Phase 3 的写入源收口才会从 command 读取它派发 diff。
    #[allow(dead_code)]
    pub unified_manager: Arc<Mutex<UnifiedWallpaperManager>>,

    /// Toast 通知窗口管理器（右下角独立通知窗口）
    pub toast_manager: Arc<Mutex<ToastManager>>,

    /// 备份操作互斥锁（防止导入/导出并发执行导致数据损坏）
    pub backup_lock: Mutex<()>,
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
        let render_coordinator = Arc::new(Mutex::new(RenderCoordinator::new(
            db.clone(),
            app_handle.clone(),
            window_manager.clone(),
        )?));
        // 启动时同步一次全局 display_mode，避免 extend 组首屏仍按 independent 渲染。
        // 用独立作用域刻意把锁收窄到这两次同步：后续创建 toast_manager 不需要持协调器锁。
        {
            let mut coordinator = render_coordinator.lock().await;
            coordinator.refresh_display_mode().await;
            coordinator.refresh_volume().await;
        }

        // Phase 2：Rust 侧一次性初始化显示器与渲染目标。
        //
        // 关键：init() 会建壁纸窗口并做 SetParent 桌面嵌入，嵌入内部用
        // SendMessageTimeoutW 与 Progman/WorkerW 通信。AppContext::new 在 setup 阶段
        // 经 block_on 同步执行，此时主窗口事件循环尚未启动，消息无人处理 → 嵌入阻塞
        // 启动（实机错误1：卡在"寻找嵌入策略"）。因此 init 移到 setup 之后由
        // setup::spawn_manager_init 异步执行，这里只构造空壳。
        let unified_manager = Arc::new(Mutex::new(UnifiedWallpaperManager::new(
            db.clone(),
            app_handle.clone(),
            render_coordinator.clone(),
            window_manager.clone(),
        )));

        let toast_manager = Arc::new(Mutex::new(ToastManager::new(app_handle.clone())));

        Ok(Self {
            window_manager,
            render_coordinator,
            unified_manager,
            toast_manager,
            app_handle,
            db,
            backup_lock: Mutex::new(()),
        })
    }

    /// 异步初始化显示器与渲染目标（setup 之后、事件循环运行时调用）。
    ///
    /// 与 `new()` 分离的原因见 `new()` 内注释：桌面嵌入依赖消息循环，
    /// 在 setup 的 block_on 里同步做会卡死启动。
    pub async fn init_manager(&self) {
        let mut manager = self.unified_manager.lock().await;
        if let Err(err) = manager.init().await {
            log::error!("[UnifiedWallpaperManager] 初始化失败: {:#}", err);
        }
    }
}
