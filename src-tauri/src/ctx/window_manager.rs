//! 壁纸窗口管理器
//!
//! 负责创建、销毁、显示/隐藏壁纸 WebviewWindow。
//! 每个物理显示器对应一个壁纸窗口，通过 URL 参数传递 monitorId。
//! Windows 上创建后会调用 desktop_embedder 嵌入桌面层级。
//!
//! 构造时注入 `AppHandle`，所有方法不再需要外部传递 app 参数。

use log::info;
use log::warn;
use std::collections::HashMap;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

#[cfg(target_os = "windows")]
use crate::platform::windows::desktop_embedder;

/// 壁纸变更事件 payload（发送给指定壁纸窗口）
#[derive(Clone, serde::Serialize)]
pub struct WallpaperChangedPayload {
    pub monitor_id: String,
    pub wallpaper_id: i32,
}

/// fitMode 变更事件 payload（发送给指定壁纸窗口）
#[derive(Clone, serde::Serialize)]
pub struct FitModeChangedPayload {
    pub monitor_id: String,
    pub fit_mode: String,
}

/// displayMode 变更事件 payload（发送给指定壁纸窗口）
#[derive(Clone, serde::Serialize)]
pub struct DisplayModeChangedPayload {
    pub monitor_id: String,
    pub display_mode: String,
}

/// 壁纸窗口管理器
pub struct WallpaperWindowManager {
    /// Tauri 应用句柄（构造时注入）
    app_handle: AppHandle,
    /// monitor_id -> window_label 映射
    windows: HashMap<String, String>,
}

impl WallpaperWindowManager {
    /// 构造 WallpaperWindowManager
    ///
    /// 注入 `AppHandle`，后续所有窗口操作均通过内部持有的句柄完成。
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            app_handle,
            windows: HashMap::new(),
        }
    }

    /// 为指定显示器创建壁纸窗口
    ///
    /// - 窗口 URL 为 /wallpaper?monitorId=xxx[&extend 参数]
    /// - 窗口属性：无边框、无任务栏、透明背景、不可缩放、初始隐藏
    /// - Windows 上创建后自动嵌入桌面层级
    pub fn create_window(
        &mut self,
        monitor_id: &str,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        extra_query: Option<&str>,
    ) -> Result<(), String> {
        let label = format!("wallpaper-{}", sanitize_label(monitor_id));

        // 如果已经存在，先销毁旧的
        if self.windows.contains_key(monitor_id) {
            self.destroy_window(monitor_id);
        }

        // 检查 Tauri 运行时中是否仍存在同 label 的窗口实例（如 F5 刷新导致的销毁延迟）
        if self.app_handle.get_webview_window(&label).is_some() {
            warn!(
                "WebviewWindow '{}' 仍存在于运行时中（可能尚未完成销毁），跳过创建",
                label
            );
            return Ok(());
        }

        let url = match extra_query {
            Some(q) if !q.is_empty() => format!("/wallpaper?monitorId={}&{}", monitor_id, q),
            _ => format!("/wallpaper?monitorId={}", monitor_id),
        };

        let window = WebviewWindowBuilder::new(&self.app_handle, &label, WebviewUrl::App(url.into()))
            .title("Wallpaper")
            .decorations(false)
            .skip_taskbar(true)
            .transparent(true)
            .resizable(false)
            .visible(false)
            .position(x as f64, y as f64)
            .inner_size(width as f64, height as f64)
            .always_on_bottom(true)
            .build()
            .map_err(|e| format!("Failed to create wallpaper window: {}", e))?;

        // Windows 平台：嵌入桌面
        #[cfg(target_os = "windows")]
        {
            if let Ok(hwnd) = window.hwnd() {
                let hwnd_isize = hwnd.0 as isize;
                if let Err(e) = desktop_embedder::embed_in_desktop(
                    hwnd_isize,
                    x,
                    y,
                    width as i32,
                    height as i32,
                ) {
                    log::error!("桌面嵌入失败: {:#}", e);
                }
            }
        }

        // 嵌入完成后显示窗口
        let _ = window.show();

        self.windows.insert(monitor_id.to_string(), label.clone());
        info!(
            "壁纸窗口已创建: label='{}', monitor='{}', pos=({}, {}), size={}x{}",
            label, monitor_id, x, y, width, height
        );

        Ok(())
    }

    /// 销毁指定显示器的壁纸窗口
    pub fn destroy_window(&mut self, monitor_id: &str) {
        if let Some(label) = self.windows.remove(monitor_id) {
            if let Some(window) = self.app_handle.get_webview_window(&label) {
                #[cfg(target_os = "windows")]
                {
                    if let Ok(hwnd) = window.hwnd() {
                        desktop_embedder::unembed_from_desktop(hwnd.0 as isize);
                    }
                }
                let _ = window.close();
            }
            info!("壁纸窗口已销毁: monitor='{}'", monitor_id);
        }
    }

    /// 销毁所有壁纸窗口
    pub fn destroy_all(&mut self) {
        let monitor_ids: Vec<String> = self.windows.keys().cloned().collect();
        for monitor_id in monitor_ids {
            self.destroy_window(&monitor_id);
        }
    }

    /// 隐藏所有壁纸窗口（全屏暂停时使用）
    pub fn hide_all(&self) {
        for label in self.windows.values() {
            if let Some(window) = self.app_handle.get_webview_window(label) {
                let _ = window.hide();
            }
        }
    }

    /// 显示所有壁纸窗口（恢复时使用）
    pub fn show_all(&self) {
        for label in self.windows.values() {
            if let Some(window) = self.app_handle.get_webview_window(label) {
                let _ = window.show();
            }
        }
    }

    /// 向所有壁纸窗口广播事件
    ///
    /// 遍历 manager 管理的全部窗口 label，逐一 emit_to，
    /// 确保事件仅发送给壁纸窗口，不会波及 main 等其他窗口。
    pub fn broadcast<S: serde::Serialize + Clone>(
        &self,
        event: &str,
        payload: &S,
    ) {
        for (monitor_id, label) in &self.windows {
            if let Err(e) = self.app_handle.emit_to(label, event, payload.clone()) {
                log::warn!(
                    "[WallpaperWindowManager] 广播事件 '{}' 到 '{}' 失败: {}",
                    event, monitor_id, e
                );
            }
        }
    }

    /// 通知指定显示器的壁纸窗口更新壁纸
    ///
    /// 通过 HashMap 精确获取 monitor_id 对应的窗口 label，
    /// 使用 emit_to 向该窗口单独发送 wallpaper-changed 事件。
    pub fn update_window(
        &self,
        monitor_id: &str,
        wallpaper_id: i32,
    ) -> Result<(), String> {
        let label = self
            .windows
            .get(monitor_id)
            .ok_or_else(|| format!("壁纸窗口不存在: monitor_id='{}'", monitor_id))?;

        // 确认窗口实例仍然存在
        let _window = self.app_handle.get_webview_window(label).ok_or_else(|| {
            format!(
                "窗口实例已丢失: label='{}', monitor_id='{}'",
                label, monitor_id
            )
        })?;

        let payload = WallpaperChangedPayload {
            monitor_id: monitor_id.to_string(),
            wallpaper_id,
        };

        self.app_handle
            .emit_to(label, "wallpaper-changed", &payload)
            .map_err(|e| format!("发送事件失败: {}", e))?;

        info!(
            "壁纸更新事件已发送: monitor='{}', wallpaper_id={}, target='{}'",
            monitor_id, wallpaper_id, label
        );

        Ok(())
    }

    /// 通知指定显示器的壁纸窗口 fitMode 变更
    ///
    /// 壁纸窗口收到后直接更新 objectFit 样式，无需重新加载壁纸数据。
    pub fn notify_fit_mode_changed(
        &self,
        monitor_id: &str,
        fit_mode: &str,
    ) -> Result<(), String> {
        let label = self
            .windows
            .get(monitor_id)
            .ok_or_else(|| format!("壁纸窗口不存在: monitor_id='{}'", monitor_id))?;

        let _window = self.app_handle.get_webview_window(label).ok_or_else(|| {
            format!(
                "窗口实例已丢失: label='{}', monitor_id='{}'",
                label, monitor_id
            )
        })?;

        let payload = FitModeChangedPayload {
            monitor_id: monitor_id.to_string(),
            fit_mode: fit_mode.to_string(),
        };

        self.app_handle
            .emit_to(label, "fit-mode-changed", &payload)
            .map_err(|e| format!("发送 fit-mode-changed 事件失败: {}", e))?;

        info!(
            "fit-mode-changed 事件已发送: monitor='{}', fit_mode='{}', target='{}'",
            monitor_id, fit_mode, label
        );

        Ok(())
    }

    /// 通知指定显示器的壁纸窗口 displayMode 变更
    ///
    /// 壁纸窗口收到后切换渲染模式（independent / mirror / extend）。
    /// extend 模式下前端通过 availableMonitors() API 自行计算裁剪区域，
    /// 无需后端传递视口参数。
    pub fn notify_display_mode_changed(
        &self,
        monitor_id: &str,
        display_mode: &str,
    ) -> Result<(), String> {
        let label = self
            .windows
            .get(monitor_id)
            .ok_or_else(|| format!("壁纸窗口不存在: monitor_id='{}'", monitor_id))?;

        let _window = self.app_handle.get_webview_window(label).ok_or_else(|| {
            format!(
                "窗口实例已丢失: label='{}', monitor_id='{}'",
                label, monitor_id
            )
        })?;

        let payload = DisplayModeChangedPayload {
            monitor_id: monitor_id.to_string(),
            display_mode: display_mode.to_string(),
        };

        self.app_handle
            .emit_to(label, "display-mode-changed", &payload)
            .map_err(|e| format!("发送 display-mode-changed 事件失败: {}", e))?;

        info!(
            "display-mode-changed 事件已发送: monitor='{}', display_mode='{}', target='{}'",
            monitor_id, display_mode, label
        );

        Ok(())
    }

    /// 获取当前管理的窗口数量
    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    /// 检查指定显示器是否已有壁纸窗口
    pub fn has_window(&self, monitor_id: &str) -> bool {
        self.windows.contains_key(monitor_id)
    }

    /// 获取当前所有已创建壁纸窗口的 monitor_id 列表
    pub fn get_active_window_ids(&self) -> Vec<String> {
        self.windows.keys().cloned().collect()
    }
}

/// 清理 monitor_id 使其适合做 Tauri window label
/// label 只允许 字母数字和 - _
fn sanitize_label(monitor_id: &str) -> String {
    monitor_id
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}