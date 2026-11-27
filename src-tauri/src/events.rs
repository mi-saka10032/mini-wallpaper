//! 事件系统：类型安全的事件发送
//!
//! 所有 Rust → 前端的事件 Payload 集中定义于此，
//! 每个 Payload 通过 `EventPayload` trait 绑定唯一事件名，
//! 调用方使用 `app.typed_emit(&payload)` 即可发送，编译期保证类型安全。

use serde::Serialize;
use tauri::{AppHandle, Emitter};

// ==================== Trait 定义 ====================

/// 事件 Payload trait：每个 Payload 自带事件名
pub trait EventPayload: Clone + Serialize {
    /// 与前端 `EVENTS` 常量一一对应的事件名
    const EVENT_NAME: &'static str;
}

/// 类型安全的事件发送 trait
///
/// 通过泛型约束自动从 Payload 类型推导事件名，
/// 不可能传错事件名，不可能传错 Payload 类型。
pub trait TypedEmit {
    fn typed_emit<P: EventPayload>(&self, payload: &P) -> Result<(), tauri::Error>;
}

impl TypedEmit for AppHandle {
    fn typed_emit<P: EventPayload>(&self, payload: &P) -> Result<(), tauri::Error> {
        self.emit(P::EVENT_NAME, payload)
    }
}

/// 类型安全的定向事件发送 trait
///
/// 与 `TypedEmit` 对应，用于向指定 label 的窗口发送事件。
/// 事件名同样从 Payload 类型自动推导，编译期保证类型安全。
pub trait TypedEmitTo {
    fn typed_emit_to<P: EventPayload>(&self, label: &str, payload: &P) -> Result<(), tauri::Error>;
}

impl TypedEmitTo for AppHandle {
    fn typed_emit_to<P: EventPayload>(&self, label: &str, payload: &P) -> Result<(), tauri::Error> {
        self.emit_to(label, P::EVENT_NAME, payload)
    }
}

// ==================== Payload 定义 ====================

/// 壁纸变更事件（通知壁纸窗口切换壁纸）
#[derive(Clone, Serialize)]
pub struct WallpaperChangedPayload {
    pub monitor_id: String,
    pub wallpaper_id: i32,
}

impl EventPayload for WallpaperChangedPayload {
    const EVENT_NAME: &'static str = "wallpaper-changed";
}

/// 缩略图变更事件（通知主窗口更新缩略图）
///
/// 与 backend 无关：无论壁纸由 Native 渲染目标还是 WebView 窗口显示，换图后
/// 都要通知主窗口刷新，否则收藏 / 设置页会显示上一张的缩略图。
#[derive(Clone, Serialize)]
pub struct ThumbnailChangedPayload {
    pub monitor_id: String,
    pub wallpaper_id: i32,
}

impl EventPayload for ThumbnailChangedPayload {
    const EVENT_NAME: &'static str = "thumbnail-changed";
}

/// 备份进度事件（导入/导出进度通知）
#[derive(Clone, Serialize)]
pub struct BackupProgressPayload {
    /// 已处理字节数
    pub current: u64,
    /// 总字节数
    pub total: u64,
}

impl EventPayload for BackupProgressPayload {
    const EVENT_NAME: &'static str = "backup-progress";
}

/// 全屏状态变更事件
#[derive(Clone, Serialize)]
pub struct FullscreenChangedPayload {
    pub is_fullscreen: bool,
}

impl EventPayload for FullscreenChangedPayload {
    const EVENT_NAME: &'static str = "fullscreen-changed";
}

/// 视频同步事件（extend 模式跨窗口帧同步）
#[derive(Clone, Serialize)]
pub struct VideoSyncPayload {
    pub current_time: f64,
}

impl EventPayload for VideoSyncPayload {
    const EVENT_NAME: &'static str = "video-sync";
}

/// 全局音量变更事件
#[derive(Clone, Serialize)]
pub struct VolumeChangedPayload {
    pub volume: f64,
}

impl EventPayload for VolumeChangedPayload {
    const EVENT_NAME: &'static str = "volume-changed";
}

/// fitMode 变更事件
#[derive(Clone, Serialize)]
pub struct FitModeChangedPayload {
    pub monitor_id: String,
    pub fit_mode: String,
}

impl EventPayload for FitModeChangedPayload {
    const EVENT_NAME: &'static str = "fit-mode-changed";
}

/// displayMode 变更事件
#[derive(Clone, Serialize)]
pub struct DisplayModeChangedPayload {
    pub monitor_id: String,
    pub display_mode: String,
}

impl EventPayload for DisplayModeChangedPayload {
    const EVENT_NAME: &'static str = "display-mode-changed";
}

/// extend 视口事件（M20：Rust 单一事实源算裁剪参数，按 monitor_id 定向推送）
///
/// 字段单位已是 **CSS 像素**：Rust 用 `GetDpiForMonitor` 拿到的目标屏真实 DPI
/// 把 `ExtendViewport` 的物理坐标换算好再推。前端直接用于 style，零计算。
/// 这同时修掉 M18 根因：WebView 嵌入桌面后 `scaleFactor()` 拿到的往往是主屏
/// DPI 而非目标屏 DPI，多屏异构时必然错位。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtendViewportPayload {
    pub offset_x: f64,
    pub offset_y: f64,
    pub total_width: f64,
    pub total_height: f64,
    pub monitor_width: f64,
    pub monitor_height: f64,
}

impl EventPayload for ExtendViewportPayload {
    const EVENT_NAME: &'static str = "extend-viewport";
}

impl ExtendViewportPayload {
    /// 从物理坐标的 `ExtendViewport` + 目标屏 DPI 构造 CSS 像素 payload。
    pub fn from_physical(
        viewport: crate::platform::wallpaper_renderer::ExtendViewport,
        dpi: u32,
    ) -> Self {
        let scale = if dpi > 0 { dpi as f64 / 96.0 } else { 1.0 };
        Self {
            offset_x: viewport.offset_x as f64 / scale,
            offset_y: viewport.offset_y as f64 / scale,
            total_width: viewport.total_width as f64 / scale,
            total_height: viewport.total_height as f64 / scale,
            monitor_width: viewport.monitor_width as f64 / scale,
            monitor_height: viewport.monitor_height as f64 / scale,
        }
    }
}

/// 壁纸清空事件（通知壁纸窗口清除当前壁纸，显示黑屏）
///
/// 当壁纸被删除且无后续壁纸可切换时，通知壁纸窗口清空显示。
#[derive(Clone, Serialize)]
pub struct WallpaperClearedPayload {
    pub monitor_id: String,
}

impl EventPayload for WallpaperClearedPayload {
    const EVENT_NAME: &'static str = "wallpaper-cleared";
}

/// 显示器配置刷新事件（通知主窗口重新拉取 config 状态）
///
/// 当后端因删除操作导致 monitor_config 发生变更（如 wallpaper_id/collection_id 被清空）时，
/// 通知主窗口刷新 store 状态，确保 UI 与 DB 一致。
#[derive(Clone, Serialize)]
pub struct MonitorConfigRefreshedPayload;

impl EventPayload for MonitorConfigRefreshedPayload {
    const EVENT_NAME: &'static str = "monitor-config-refreshed";
}

/// 应用设置刷新事件（场景恢复等后端直接改 app_settings 后通知主窗口重取）
///
/// 托盘点击场景不经过前端 invoke，因此不能用 COMMAND_SIGNALS 覆盖，
/// 必须由后端发事件；monitor-config-refreshed 走同一条链路。
#[derive(Clone, Serialize)]
pub struct AppSettingsRefreshedPayload;

impl EventPayload for AppSettingsRefreshedPayload {
    const EVENT_NAME: &'static str = "app-settings-refreshed";
}

/// 动作反馈 Toast 事件（通知主窗口显示操作反馈）
///
/// 由 `TaskRegistry::dispatch_action` 在动作执行完成后发送，
/// 前端监听后通过 sonner toast 显示短暂的操作反馈。
#[derive(Clone, Serialize)]
pub struct ActionToastPayload {
    /// 动作标识（对应 Action 枚举的 camelCase 名称，如 "next"、"togglePause"）
    pub action: String,
    /// 人类可读的反馈消息
    pub message: String,
}

impl EventPayload for ActionToastPayload {
    const EVENT_NAME: &'static str = "action-toast";
}

/// 收藏夹归属变更事件（通知主窗口刷新收藏状态）
///
/// 当壁纸被加入 / 移出内置「我喜欢」收藏夹（ToggleFavorite 快捷键 / 托盘 / 红心按钮）
/// 时发送，前端据此刷新收藏 id 集合，红心按钮亮灭与「我喜欢」列表即时同步。
#[derive(Clone, Serialize)]
pub struct FavoritesChangedPayload {
    /// 被切换的壁纸 ID
    pub wallpaper_id: i32,
    /// 切换后的状态：true = 已收藏，false = 已取消收藏
    pub favorited: bool,
}

impl EventPayload for FavoritesChangedPayload {
    const EVENT_NAME: &'static str = "favorites-changed";
}

/// 壁纸渲染器错误 / 降级提示事件（发往主窗口）
///
/// 这是「发后不理」的单向通知：后端只负责 `typed_emit`，不等待前端确认
/// （无 ack 回执），同一错误可能重复上报，前端仅用 toast 即时呈现，不据此
/// 驱动业务状态。注意 `code` 不是枚举而是稳定字符串：渲染错误取
/// `RendererErrorCode::as_str()`（如 UnsupportedFormat / DecodeFailed），
/// 而 `ExtendDegraded` 是 extend 组降级的专用提示码，前端据此显示 warning
/// 而非 error。`monitor_id` / `wallpaper_id` 用 Option 是因为降级等事件是
/// 全局性的，不存在明确的出错显示器或壁纸。
#[derive(Clone, Serialize)]
pub struct RendererErrorPayload {
    /// 出错显示器；全局性事件（如 extend 降级）为 None
    pub monitor_id: Option<String>,
    /// 出错壁纸；无法定位到具体壁纸时为 None
    pub wallpaper_id: Option<i32>,
    /// 稳定错误码字符串，供前端区分能力边界 / 运行时故障 / 降级
    pub code: String,
    /// 人类可读信息，供日志与 toast 展示
    pub message: String,
}

impl EventPayload for RendererErrorPayload {
    const EVENT_NAME: &'static str = "renderer-error";
}
