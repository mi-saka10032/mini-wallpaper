//! 应用上下文模块
//!
//! `context` 目录聚合了所有与应用生命周期一致的全局基础设施组件（纯状态容器）：
//!
//! - **`AppContext`**：全局状态的唯一聚合入口
//! - **`db`**：数据库连接初始化与迁移
//! - **`unified_manager`**：壁纸渲染目标的统一管理器
//! - **`play_queue`**：随机播放的环形洗牌牌堆（纯内存运行时状态）
//!
//! 运行时调度（轮播定时器、全屏检测等后台任务）已迁移至 `runtime/` 模块，
//! `TaskRegistry` 作为独立的全局 state 与 `AppContext` 平级注册。
//!
//! ## 设计原则
//! - **单一入口**：一个 `AppContext` 贯穿全局，替代多个独立 managed state
//! - **控制反转**：所有子组件在 `new` 时注入 `AppHandle`，方法签名不再传递句柄
//! - **职责分离**：`context` 管"是什么"（状态），`runtime` 管"做什么"（调度）

mod db;
pub mod play_queue;
pub mod toast_manager;
/// 统一壁纸管理器：主体 + native（渲染链路）/ webview（窗口链路）两个 impl 文件
pub mod unified_manager;

use anyhow::Result;
use sea_orm::DatabaseConnection;
use tokio::sync::Mutex;

use play_queue::PlayQueueStore;
use toast_manager::ToastManager;
use unified_manager::UnifiedWallpaperManager;

/// 应用全局上下文
///
/// 聚合所有需要跨模块共享的全局状态。Tauri 的 `manage()` 已经用 `Arc` 持有
/// 整个 `AppContext`，因此字段内不再套一层 `Arc<Mutex<T>>`：所有需要跨
/// `'static` 边界的地方都持有 `AppHandle`，经 `app.state::<Arc<AppContext>>()` 即可
/// 访问同一实例。command 层通过 `State<'_, Arc<AppContext>>` 注入。
///
/// 所有子组件在构造时注入 `AppHandle`，方法调用不再需要外部传递句柄。
pub struct AppContext {
    /// Tauri 应用句柄（用于 emit 事件、获取路径等）
    pub app_handle: tauri::AppHandle,

    /// SeaORM 数据库连接
    pub db: DatabaseConnection,

    /// 统一壁纸管理器：App 的唯一主体 manager（原 RenderCoordinator /
    /// WallpaperWindowManager 已拉平并入）。
    ///
    /// reconcile 串行入口、TaskRegistry 与各 command 共享同一实例；所有壁纸状态
    /// （Native backends / WebView 窗口 / 显示器 slot / 快照）收在这一把锁下。
    pub unified_manager: Mutex<UnifiedWallpaperManager>,

    /// Toast 通知窗口管理器（右下角独立通知窗口）
    pub toast_manager: Mutex<ToastManager>,

    /// 备份操作互斥锁（防止导入/导出并发执行导致数据损坏）
    pub backup_lock: Mutex<()>,

    /// 随机播放牌堆：key 为 `carousel_key(monitor_id)`，与轮播 timer 对齐。
    ///
    /// 用同步锁是因为临界区只有 Vec 洗牌与 cursor 移动、零 await；
    /// 同步锁也让 `AppEffects::stop` 能在停止 timer 时顺手删条目，
    /// 避免显示器热插拔后 map 只增不删。
    pub play_queues: std::sync::Mutex<PlayQueueStore>,

    /// 目录监听：正在同步的 folder_id 集合（per-folder 互斥，临界区零 await）。
    pub watched_syncing: std::sync::Mutex<std::collections::HashSet<i32>>,

    /// 当前主窗口 active 的收藏夹 id（0 = 本地壁纸库）。
    /// 后台轮询据此跳过 active 夹的完整 diff，避免浏览中被强制刷新打断；
    /// 存活探测不受它影响。
    pub active_collection_id: std::sync::atomic::AtomicI32,
}

impl AppContext {
    /// 构造 AppContext
    ///
    /// 内部通过 `db` 模块完成数据库初始化（连接 + 迁移），
    /// 并将 `app_handle` clone 注入到各子组件，
    /// 实现子组件自持句柄、方法签名零句柄传递。
    pub async fn new(app_handle: tauri::AppHandle) -> Result<Self> {
        let db = db::init_db(&app_handle).await?;
        let unified_manager =
            Mutex::new(UnifiedWallpaperManager::new(db.clone(), app_handle.clone()).await?);

        // 关键：init() 会建壁纸窗口并做 SetParent 桌面嵌入，嵌入内部用
        // SendMessageTimeoutW 与 Progman/WorkerW 通信。AppContext::new 在 setup 阶段
        // 经 block_on 同步执行，此时主窗口事件循环尚未启动 → 嵌入阻塞启动。
        // 因此 init 移到 setup 之后由 setup::spawn_manager_init 异步执行，这里只构造空壳。

        let toast_manager = Mutex::new(ToastManager::new(app_handle.clone()));

        Ok(Self {
            unified_manager,
            toast_manager,
            app_handle,
            db,
            backup_lock: Mutex::new(()),
            play_queues: std::sync::Mutex::new(PlayQueueStore::default()),
            watched_syncing: std::sync::Mutex::new(std::collections::HashSet::new()),
            active_collection_id: std::sync::atomic::AtomicI32::new(0),
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

    /// 取牌堆锁；单次 panic 污染不应让整个轮播链路连锁 panic，故恢复内层值。
    pub fn play_queues(&self) -> std::sync::MutexGuard<'_, PlayQueueStore> {
        self.play_queues
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
