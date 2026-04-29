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