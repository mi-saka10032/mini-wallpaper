//! WebView 窗口链路 impl
//!
//! 原 `WallpaperWindowManager` 的全部字段与方法原样搬入 `UnifiedWallpaperManager`，
//! 本文件只承载"WebView 窗口怎么创建/销毁/发事件"的 impl 块。

use log::info;
use log::warn;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

use super::UnifiedWallpaperManager;

#[cfg(target_os = "windows")]
use crate::platform::windows::desktop_embedder::{self, EmbedPolicy};

use crate::events::{
    DisplayModeChangedPayload, EventPayload, FitModeChangedPayload, MonitorConfigRefreshedPayload,
    TypedEmit, TypedEmitTo, WallpaperChangedPayload, WallpaperClearedPayload,
};

/// 把物理像素矩形换算成 Tauri 建窗所需的逻辑像素。
///
/// `WebviewWindowBuilder::position` / `inner_size` 接收逻辑像素（Tauri 2.10 源码确认），
/// 而 `MonitorRect` 与 Win32 桌面嵌入使用的都是物理像素。高 DPI 屏下若直接传物理值，
/// WebView 的 CSS 视口会被放大 scale 倍，extend 裁剪随之错位。
/// 桌面嵌入阶段仍用物理坐标 SetWindowPos，不受这里影响。
///
/// 注意：坐标为虚拟桌面绝对原点（可能为负），多屏不同 DPI 时严格换算需要按屏原点
/// 分别处理；这里用目标屏自身 scale 做整体除法，覆盖单窗口只落在一块屏的常规场景。
fn logical_rect(x: i32, y: i32, width: u32, height: u32, scale: f64) -> (f64, f64, f64, f64) {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    (
        x as f64 / scale,
        y as f64 / scale,
        width as f64 / scale,
        height as f64 / scale,
    )
}

/// 查询目标屏的 DPI 缩放系数；枚举不到或非 Windows 平台回落 1.0。
/// 每次建窗枚举一次显示器（通常 1~4 块），成本相对建窗可忽略。
#[cfg(target_os = "windows")]
fn monitor_scale_factor(monitor_id: &str) -> f64 {
    crate::platform::windows::monitor_geometry::enumerate_monitors()
        .iter()
        .find(|info| info.rect.device_name == monitor_id)
        .map(|info| info.scale_factor())
        .unwrap_or(1.0)
}

#[cfg(not(target_os = "windows"))]
fn monitor_scale_factor(_monitor_id: &str) -> f64 {
    1.0
}

/// 建窗结果，供调用方在锁外完成嵌入、再回锁显示窗口
pub struct CreatedWindow {
    pub label: String,
    #[cfg(target_os = "windows")]
    pub embed_target: Option<EmbedTarget>,
}

/// Windows 桌面嵌入所需的目标信息，全部 Send，可安全移入阻塞线程
#[cfg(target_os = "windows")]
pub struct EmbedTarget {
    pub hwnd: isize,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl UnifiedWallpaperManager {
    /// 建窗阶段（持锁）：创建窗口并返回稍后需要嵌入的目标。
    ///
    /// 桌面嵌入含最长 1 秒的 Win32 同步调用，不能在本方法内执行，否则会
    /// 让调用方持有的 wm 锁跨越这段耗时。嵌入与 show 由调用方在锁外编排。
    /// async：同 label 窗口残留时需等待 Tauri 运行时完成销毁（热插拔恢复场景）。
    pub async fn create_window(
        &mut self,
        monitor_id: &str,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        extra_query: Option<&str>,
    ) -> Result<Option<CreatedWindow>, String> {
        let label = wallpaper_window_label(monitor_id);

        // 如果已经存在，先销毁旧的
        if self.windows.contains_key(monitor_id) {
            self.destroy_window(monitor_id);
        }

        // Tauri 运行时中可能仍残留同 label 的窗口实例：
        // `window.close()` 是异步的，热插拔断开→恢复（实机错误2）时，
        // 断开已把窗口从 self.windows 移除、WebView2 进程也已销毁，
        // 但 Tauri 的窗口表尚未清理。旧代码直接跳过创建，导致该屏永不绘制。
        // 这里主动再 close 一次并等待运行时真正移除，超时才报错让上层重试。
        if self.app_handle.get_webview_window(&label).is_some() {
            warn!(
                "WebviewWindow '{}' 仍存在于运行时中（可能尚未完成销毁），主动关闭并等待",
                label
            );
            if let Some(window) = self.app_handle.get_webview_window(&label) {
                let _ = window.close();
            }
            let mut gone = false;
            for _ in 0..20 {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                if self.app_handle.get_webview_window(&label).is_none() {
                    gone = true;
                    break;
                }
            }
            if !gone {
                return Err(format!("壁纸窗口 '{}' 销毁超时，无法重建", label));
            }
        }

        let url = match extra_query {
            Some(q) if !q.is_empty() => {
                format!("/wallpaper.html?monitorId={}&{}", monitor_id, q)
            }
            _ => format!("/wallpaper.html?monitorId={}", monitor_id),
        };

        // builder 只收逻辑像素；用 Win32 查到的目标屏 scale 换算物理矩形。
        // embed_target 仍保留物理值，桌面嵌入的 SetWindowPos 用的是物理坐标。
        let scale = monitor_scale_factor(monitor_id);
        let (logical_x, logical_y, logical_w, logical_h) = logical_rect(x, y, width, height, scale);
        let window =
            WebviewWindowBuilder::new(&self.app_handle, &label, WebviewUrl::App(url.into()))
                .title("Wallpaper")
                .decorations(false)
                .skip_taskbar(true)
                .transparent(true)
                .resizable(false)
                .visible(false)
                .position(logical_x, logical_y)
                .inner_size(logical_w, logical_h)
                .always_on_bottom(true)
                .build()
                .map_err(|e| format!("Failed to create wallpaper window: {}", e))?;

        #[cfg(target_os = "windows")]
        let embed_target = window.hwnd().ok().map(|hwnd| EmbedTarget {
            hwnd: hwnd.0 as isize,
            x,
            y,
            width: width as i32,
            height: height as i32,
        });
        #[cfg(not(target_os = "windows"))]
        let _ = &window;

        self.windows.insert(monitor_id.to_string(), label.clone());
        info!(
            "壁纸窗口已创建: label='{}', monitor='{}', physical=({}, {}) {}x{}, logical=({:.1}, {:.1}) {:.1}x{:.1}, dpi_scale={:.2}",
            label,
            monitor_id,
            x,
            y,
            width,
            height,
            logical_x,
            logical_y,
            logical_w,
            logical_h,
            scale
        );

        #[cfg(target_os = "windows")]
        return Ok(Some(CreatedWindow {
            label,
            embed_target,
        }));
        #[cfg(not(target_os = "windows"))]
        return Ok(Some(CreatedWindow { label }));
    }

    /// 收尾阶段（持锁）：嵌入完成后显示窗口
    pub fn show_window(&self, label: &str) {
        if let Some(window) = self.app_handle.get_webview_window(label) {
            let _ = window.show();
        }
    }

    /// 完整建窗流程：建窗 → 锁外嵌入桌面 → 显示。
    ///
    /// 供 RenderCoordinator 在 WebView backend 与跨 backend 切换时复用，
    /// 避免调用方重复编排三段式流程。
    pub async fn create_and_show(
        &mut self,
        monitor_id: &str,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let created = self
            .create_window(monitor_id, x, y, width, height, None)
            .await?
            .ok_or_else(|| format!("壁纸窗口 {} 已存在，跳过创建", monitor_id))?;

        #[cfg(target_os = "windows")]
        if let Some(target) = created.embed_target {
            let result = tauri::async_runtime::spawn_blocking(move || {
                desktop_embedder::embed_in_desktop(
                    target.hwnd,
                    target.x,
                    target.y,
                    target.width,
                    target.height,
                    EmbedPolicy::WebView,
                )
            })
            .await;
            match result {
                Ok(Err(e)) => log::error!("桌面嵌入失败: {:#}", e),
                Err(e) => log::error!("桌面嵌入任务异常: {}", e),
                Ok(Ok(())) => {}
            }
        }

        self.show_window(&created.label);
        Ok(())
    }

    /// 销毁指定显示器的壁纸窗口
    ///
    /// 先 `hide()` 再解除嵌入 / 关闭：`ShowWindow(SW_HIDE)` 会把窗口整体从
    /// DWM 合成树（含硬件视频叠加平面）里摘除，比单纯的 Z-order 更强——
    /// 跨 backend 切换时旧窗口仍在播放视频，若它的 `<video>` 走硬件叠加
    /// 渲染，叠加平面不一定严格遵守普通窗口 Z-order，只靠"被新窗口盖住"
    /// 不能保证销毁瞬间彻底不可见；显式隐藏从根上避免这段切换瞬间的闪烁。
    pub fn destroy_window(&mut self, monitor_id: &str) {
        if let Some(label) = self.windows.remove(monitor_id) {
            if let Some(window) = self.app_handle.get_webview_window(&label) {
                let _ = window.hide();
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

    /// 销毁全部壁纸窗口（设备丢失时的内存清理）。
    ///
    /// DeviceLost 只会清掉 renderer 侧 D3D11 资源，WebView2 也走 GPU 合成，
    /// 其渲染进程同样可能受影响；从内存管理角度应一并释放。
    /// 直接 close 而不走 reconcile：DB 未变化，diff 产不出销毁动作。
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
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
    /// 泛型约束 `EventPayload`，自动从 Payload 类型推导事件名，
    /// 确保事件仅发送给壁纸窗口，不会波及 main 等其他窗口。
    pub fn broadcast<P: EventPayload>(&self, payload: &P) {
        for (monitor_id, label) in &self.windows {
            if let Err(e) = self.app_handle.typed_emit_to(label, payload) {
                log::warn!(
                    "[WallpaperWindowManager] 广播事件 '{}' 到 '{}' 失败: {}",
                    P::EVENT_NAME,
                    monitor_id,
                    e
                );
            }
        }
    }

    /// 解析 monitor_id 对应的窗口实例
    ///
    /// 统一的窗口查找逻辑：先从 HashMap 获取 label，再从 Tauri 运行时获取窗口实例。
    /// 消除 update_window / notify_fit_mode_changed / notify_display_mode_changed 中的重复代码。
    fn resolve_window(&self, monitor_id: &str) -> Result<(String, tauri::WebviewWindow), String> {
        let label = self
            .windows
            .get(monitor_id)
            .ok_or_else(|| format!("壁纸窗口不存在: monitor_id='{}'", monitor_id))?;

        let window = self.app_handle.get_webview_window(label).ok_or_else(|| {
            format!(
                "窗口实例已丢失: label='{}', monitor_id='{}'",
                label, monitor_id
            )
        })?;

        Ok((label.clone(), window))
    }

    /// 通知指定显示器的壁纸窗口更新壁纸
    ///
    /// 通过 HashMap 精确获取 monitor_id 对应的窗口 label，
    /// 使用 emit_to 向该窗口单独发送 wallpaper-changed 事件。
    pub fn update_window(&self, monitor_id: &str, wallpaper_id: i32) -> Result<(), String> {
        let (label, _window) = self.resolve_window(monitor_id)?;

        let payload = WallpaperChangedPayload {
            monitor_id: monitor_id.to_string(),
            wallpaper_id,
        };

        self.app_handle
            .typed_emit_to(&label, &payload)
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
    pub fn notify_fit_mode_changed(&self, monitor_id: &str, fit_mode: &str) -> Result<(), String> {
        let (label, _window) = self.resolve_window(monitor_id)?;

        let payload = FitModeChangedPayload {
            monitor_id: monitor_id.to_string(),
            fit_mode: fit_mode.to_string(),
        };

        self.app_handle
            .typed_emit_to(&label, &payload)
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
        let (label, _window) = self.resolve_window(monitor_id)?;

        let payload = DisplayModeChangedPayload {
            monitor_id: monitor_id.to_string(),
            display_mode: display_mode.to_string(),
        };

        self.app_handle
            .typed_emit_to(&label, &payload)
            .map_err(|e| format!("发送 display-mode-changed 事件失败: {}", e))?;

        info!(
            "display-mode-changed 事件已发送: monitor='{}', display_mode='{}', target='{}'",
            monitor_id, display_mode, label
        );

        Ok(())
    }

    /// 广播 displayMode 变更，只发给**实际存在**的 WebView 窗口。
    ///
    /// 纯 Native 组没有任何 label，按 active_configs 逐个 emit 会全部
    /// `resolve_window` 失败刷 warn；以已创建窗口为准即可。
    pub fn broadcast_display_mode_changed(&self, display_mode: &str) {
        for monitor_id in self.windows.keys() {
            if let Err(e) = self.notify_display_mode_changed(monitor_id, display_mode) {
                log::warn!(
                    "[WallpaperWindowManager] display-mode-changed 发送失败 {}: {}",
                    monitor_id,
                    e
                );
            }
        }
    }

    /// 通知指定壁纸窗口清空壁纸显示
    ///
    /// 当壁纸被删除且无后续壁纸可切换时，通知壁纸窗口清除当前壁纸（显示黑屏）。
    pub fn clear_window(&self, monitor_id: &str) -> Result<(), String> {
        let (label, _window) = self.resolve_window(monitor_id)?;

        let payload = WallpaperClearedPayload {
            monitor_id: monitor_id.to_string(),
        };

        self.app_handle
            .typed_emit_to(&label, &payload)
            .map_err(|e| format!("发送 wallpaper-cleared 事件失败: {}", e))?;

        info!(
            "wallpaper-cleared 事件已发送: monitor='{}', target='{}'",
            monitor_id, label
        );

        Ok(())
    }

    /// display_mode 感知的壁纸清空通知
    ///
    /// 根据 is_sync_mode 决定广播或单播：
    /// - 同步模式（mirror/extend）：清空所有壁纸窗口
    /// - 独立模式（independent）：仅清空指定 monitor 的壁纸窗口
    pub fn notify_wallpaper_cleared(&self, monitor_id: &str, is_sync_mode: bool) {
        if is_sync_mode {
            for mid in self.windows.keys() {
                if let Err(e) = self.clear_window(mid) {
                    log::warn!(
                        "[WallpaperWindowManager] 同步模式壁纸清空失败 {}: {}",
                        mid,
                        e
                    );
                }
            }
        } else if let Err(e) = self.clear_window(monitor_id) {
            log::warn!(
                "[WallpaperWindowManager] 壁纸清空失败 {}: {}",
                monitor_id,
                e
            );
        }
    }

    /// 通知主窗口刷新 config 状态
    ///
    /// 当后端因删除操作导致 monitor_config 发生变更时，
    /// 通知主窗口重新拉取 store 状态，确保 UI 与 DB 一致。
    pub fn notify_config_refreshed(&self) {
        if let Err(e) = self.app_handle.typed_emit(&MonitorConfigRefreshedPayload) {
            log::warn!(
                "[WallpaperWindowManager] 发送 monitor-config-refreshed 失败: {}",
                e
            );
        }
    }
}

/// 按 monitor_id 推导壁纸窗口 label（与 create_window 使用同一规则）。
/// 供 Manager 定向推送事件使用，避免两处各写一份命名逻辑。
pub fn wallpaper_window_label(monitor_id: &str) -> String {
    format!("wallpaper-{}", sanitize_label(monitor_id))
}

#[cfg(test)]
mod tests {
    use super::logical_rect;

    #[test]
    fn logical_rect_is_identity_at_100_percent() {
        assert_eq!(
            logical_rect(100, 50, 1920, 1080, 1.0),
            (100.0, 50.0, 1920.0, 1080.0)
        );
    }

    #[test]
    fn logical_rect_divides_by_scale_for_high_dpi() {
        assert_eq!(
            logical_rect(0, 0, 3840, 2160, 2.0),
            (0.0, 0.0, 1920.0, 1080.0)
        );
        assert_eq!(
            logical_rect(-1920, 100, 1920, 1080, 1.5),
            (-1280.0, 100.0 / 1.5, 1280.0, 720.0)
        );
    }

    #[test]
    fn logical_rect_falls_back_for_invalid_scale() {
        assert_eq!(
            logical_rect(10, 20, 800, 600, 0.0),
            (10.0, 20.0, 800.0, 600.0)
        );
        assert_eq!(
            logical_rect(10, 20, 800, 600, f64::NAN),
            (10.0, 20.0, 800.0, 600.0)
        );
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
