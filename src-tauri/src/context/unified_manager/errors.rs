//! 渲染失败与设备丢失的上层出口
//!
//! 渲染层（`desktop_renderer`）只负责本机 GPU 资源清理：检测到设备丢失时通过
//! `DeviceLostHandler` 回调通知上层，命令执行失败则经 oneshot 回执返回 `Err`。
//! 本模块把两类失败在 UM 侧的收尾集中起来：
//! - **通用失败**：`emit_renderer_error` 把 `RendererErrorPayload` 发成 renderer-error 事件；
//! - **设备丢失**：`device_lost_handler` 构造交给 `DesktopRenderer::start` 的回调，
//!   除上报事件外还要清理 WebView2 壁纸窗口（M19）。
//!
//! 这样渲染层完全不依赖 Tauri / AppContext，依赖方向保持单向；两处失败也共用
//! 同一条 emit 出口，不会各拼一份 payload / 各写一遍发送失败日志。

use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::context::AppContext;
use crate::events::{RendererErrorPayload, TypedEmit};
use crate::platform::wallpaper_renderer::{DeviceLostHandler, RendererErrorCode};

/// 通用失败出口：向前端 emit 一次 renderer-error。
///
/// 事件发送失败只记日志——渲染链路已失败，不应因通知失败再改变控制流。
/// 供 `native.rs::report_error`（命令回执失败 / extend 降级）与设备丢失回调共用。
pub(super) fn emit_renderer_error(app_handle: &AppHandle, payload: RendererErrorPayload) {
    if let Err(e) = app_handle.typed_emit(&payload) {
        log::warn!(
            "[UnifiedWallpaperManager] renderer-error 事件发送失败: {}",
            e
        );
    }
}

/// 构造设备丢失回调，交给 `DesktopRenderer::start`。
///
/// 渲染线程只做本机 GPU 资源清理；这里负责两件上层收尾：
/// 1. 上报 DeviceLost 事件（走通用失败出口）；
/// 2. M19：WebView2 同样走 GPU 合成，设备丢失时一并释放其壁纸窗口。
///    窗口销毁由下一次 ensure_target 命令重建，不主动恢复。
///
/// 闭包是 `Fn`（设备丢失可能多次触发），因此只能 clone 句柄给 spawn，
/// 不能 move 捕获值。回调只捕获 `AppHandle`（'static），不持有 UM 本身。
pub(super) fn device_lost_handler(app_handle: &AppHandle) -> DeviceLostHandler {
    let emit_handle = app_handle.clone();
    let cleanup_handle = app_handle.clone();
    Arc::new(move || {
        emit_renderer_error(
            &emit_handle,
            RendererErrorPayload {
                monitor_id: None,
                wallpaper_id: None,
                code: RendererErrorCode::DeviceLost.as_str().to_string(),
                message: "显卡设备丢失，原生壁纸已清理；下次换图将自动恢复".to_string(),
            },
        );
        // unified_manager 是 tokio Mutex，清理要 spawn 到 async runtime 执行。
        let cleanup = cleanup_handle.clone();
        tauri::async_runtime::spawn(async move {
            if let Some(ctx) = cleanup.try_state::<AppContext>() {
                let app_ctx: &AppContext = ctx.inner();
                app_ctx.unified_manager.lock().await.destroy_all();
                log::info!("[UnifiedWallpaperManager] DeviceLost 已清理全部 WebView 壁纸窗口");
            }
        });
    })
}
