//! 壁纸窗口管理服务
//!
//! 负责创建、销毁、显示/隐藏壁纸 WebviewWindow。
//! 每个物理显示器对应一个壁纸窗口，通过 URL 参数传递 monitorId。
//! Windows 上创建后会调用 desktop_embedder 嵌入桌面层级。

use log::{error, info, warn};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::Mutex;

#[cfg(target_os = "windows")]
use crate::platform::windows::desktop_embedder;

/// 壁纸变更事件 payload（发送给指定壁纸窗口）
#[derive(Clone, serde::Serialize)]
pub struct WallpaperChangedPayload {
    pub monitor_id: String,
    pub wallpaper_id: i32,
}

/// 壁纸窗口管理器 managed state
pub type WallpaperWindowManagerState = Arc<Mutex<WallpaperWindowManager>>;

/// 创建壁纸窗口管理器
pub fn create_wallpaper_window_manager() -> WallpaperWindowManagerState {
    Arc::new(Mutex::new(WallpaperWindowManager::new()))
}

/// 壁纸窗口管理器
pub struct WallpaperWindowManager {
    /// monitor_id -> window_label 映射
    windows: HashMap<String, String>,
}

impl WallpaperWindowManager {
    pub fn new() -> Self {
        Self {
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
        app: &AppHandle,
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
            self.destroy_window(app, monitor_id);
        }

        let url = match extra_query {
            Some(q) if !q.is_empty() => format!("/wallpaper?monitorId={}&{}", monitor_id, q),
            _ => format!("/wallpaper?monitorId={}", monitor_id),
        };

        let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
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
                    // anyhow::Error 使用 {:#} 输出完整错误链
                    error!("桌面嵌入失败: {:#}", e);
                    // 嵌入失败不阻止窗口创建，壁纸仍然可以显示为普通窗口
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
    pub fn destroy_window(&mut self, app: &AppHandle, monitor_id: &str) {
        if let Some(label) = self.windows.remove(monitor_id) {
            if let Some(window) = app.get_webview_window(&label) {
                // Windows：先从桌面解除嵌入
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
    pub fn destroy_all(&mut self, app: &AppHandle) {
        let monitor_ids: Vec<String> = self.windows.keys().cloned().collect();
        for monitor_id in monitor_ids {
            self.destroy_window(app, &monitor_id);
        }
    }

    /// 隐藏所有壁纸窗口（全屏暂停时使用）
    pub fn hide_all(&self, app: &AppHandle) {
        for label in self.windows.values() {
            if let Some(window) = app.get_webview_window(label) {
                let _ = window.hide();
            }
        }
    }

    /// 显示所有壁纸窗口（恢复时使用）
    pub fn show_all(&self, app: &AppHandle) {
        for label in self.windows.values() {
            if let Some(window) = app.get_webview_window(label) {
                let _ = window.show();
            }
        }
    }

    /// 通知指定显示器的壁纸窗口更新壁纸
    ///
    /// 通过 HashMap 精确获取 monitor_id 对应的窗口 label，
    /// 使用 emit_to 向该窗口单独发送 wallpaper-changed 事件。
    /// 相比全局广播，这种方式实现了逻辑解耦，也更适合窗口独立设置壁纸的场景。
    pub fn update_window(
        &self,
        app: &AppHandle,
        monitor_id: &str,
        wallpaper_id: i32,
    ) -> Result<(), String> {
        let label = self
            .windows
            .get(monitor_id)
            .ok_or_else(|| format!("壁纸窗口不存在: monitor_id='{}'", monitor_id))?;

        // 确认窗口实例仍然存在
        let _window = app.get_webview_window(label).ok_or_else(|| {
            format!(
                "窗口实例已丢失: label='{}', monitor_id='{}'",
                label, monitor_id
            )
        })?;

        let payload = WallpaperChangedPayload {
            monitor_id: monitor_id.to_string(),
            wallpaper_id,
        };

        app.emit_to(label, "wallpaper-changed", &payload)
            .map_err(|e| format!("发送事件失败: {}", e))?;

        info!(
            "壁纸更新事件已发送: monitor='{}', wallpaper_id={}, target='{}'",
            monitor_id, wallpaper_id, label
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
