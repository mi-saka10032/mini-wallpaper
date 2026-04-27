//! 全屏检测模块
//!
//! 提供 Windows 平台的全屏应用检测功能。
//! 本模块不持有定时器句柄，仅提供：
//! - `check_fullscreen()`: 纯检测函数，返回当前是否有全屏应用
//! - `spawn_detection_task()`: 工厂函数，创建轮询检测异步任务并返回 JoinHandle
//!
//! 定时器生命周期由 `TimerRegistry` 统一管理。

use std::time::Duration;

use log::info;
use tauri::Emitter;
use tokio::task::JoinHandle;

/// 全屏检测定时器在 TimerRegistry 中的 key
pub const FULLSCREEN_TIMER_KEY: &str = "fullscreen_detector";

/// 全屏状态变更事件 payload
#[derive(Clone, serde::Serialize)]
pub struct FullscreenChangedPayload {
    pub is_fullscreen: bool,
}

/// 创建全屏检测轮询任务（工厂函数）
///
/// 返回 `JoinHandle`，由调用方注册到 `TimerRegistry`。
/// 任务内部每 2 秒检测一次全屏状态，状态变化时 emit 事件通知前端。
pub fn spawn_detection_task(app_handle: tauri::AppHandle) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut was_fullscreen = false;

        loop {
            let is_fullscreen = check_fullscreen();

            if is_fullscreen != was_fullscreen {
                if is_fullscreen {
                    info!("检测到全屏应用 — 暂停壁纸");
                } else {
                    info!("全屏应用已退出 — 恢复壁纸");
                }

                let _ = app_handle.emit(
                    "fullscreen-changed",
                    FullscreenChangedPayload { is_fullscreen },
                );
                was_fullscreen = is_fullscreen;
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    })
}

/// Windows 全屏检测实现
///
/// 通过 GetForegroundWindow 获取前台窗口，再用 GetWindowRect 获取窗口矩形，
/// 与所在显示器的屏幕尺寸比对，判断是否为全屏应用。
/// 排除桌面窗口（Shell_TrayWnd / Progman / WorkerW）避免误判。
#[cfg(target_os = "windows")]
fn check_fullscreen() -> bool {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowRect, GetClassNameW, GetDesktopWindow,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        MonitorFromWindow, GetMonitorInfoW, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };

    unsafe {
        let fg_hwnd = GetForegroundWindow();
        if fg_hwnd == 0 || fg_hwnd == GetDesktopWindow() {
            return false;
        }

        // 排除系统桌面相关窗口类
        let mut class_name = [0u16; 256];
        let len = GetClassNameW(fg_hwnd, class_name.as_mut_ptr(), 256);
        if len > 0 {
            let name = String::from_utf16_lossy(&class_name[..len as usize]);
            // Shell_TrayWnd = 任务栏, Progman / WorkerW = 桌面
            if matches!(name.as_str(), "Shell_TrayWnd" | "Progman" | "WorkerW") {
                return false;
            }
        }

        // 获取前台窗口矩形
        let mut window_rect: RECT = std::mem::zeroed();
        if GetWindowRect(fg_hwnd, &mut window_rect) == 0 {
            return false;
        }

        // 获取窗口所在显示器的信息
        let monitor = MonitorFromWindow(fg_hwnd, MONITOR_DEFAULTTONEAREST);
        if monitor == 0 {
            return false;
        }

        let mut monitor_info: MONITORINFO = std::mem::zeroed();
        monitor_info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut monitor_info) == 0 {
            return false;
        }

        let screen = monitor_info.rcMonitor;

        // 窗口矩形覆盖整个显示器区域即视为全屏
        window_rect.left <= screen.left
            && window_rect.top <= screen.top
            && window_rect.right >= screen.right
            && window_rect.bottom >= screen.bottom
    }
}

/// 非 Windows 平台：全屏检测不可用，始终返回 false
#[cfg(not(target_os = "windows"))]
fn check_fullscreen() -> bool {
    false
}
