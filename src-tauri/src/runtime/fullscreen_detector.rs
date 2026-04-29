//! 全屏检测模块
//!
//! 提供全屏应用检测功能，Windows 平台通过 Win32 API 实现，
//! 非 Windows 平台始终返回 false（无全屏检测需求）。
//!
//! 本模块不持有定时器句柄，仅提供：
//! - `check_fullscreen()`: 纯检测函数，返回当前是否有全屏应用
//! - `FullscreenDetectionTask`: 任务定义，实现 `TaskSpawner` trait
//!
//! 定时器生命周期由 `Scheduler` 统一管理，
//! `FullscreenDetectionTask` 为零字段单元结构体，`spawn` 接收调度器注入的 `AppHandle`。

use std::time::Duration;

use log::info;
use tokio::task::JoinHandle;

use super::scheduler::TaskSpawner;
use crate::events::{FullscreenChangedPayload, TypedEmit};

/// 全屏检测定时器在 Scheduler 中的 key
pub const FULLSCREEN_TIMER_KEY: &str = "fullscreen_detector";

/// 全屏检测任务定义
///
/// 零字段单元结构体，`spawn` 接收调度器注入的 `AppHandle`（用于 emit 事件），
/// 无需外部注入任何其他依赖。
pub struct FullscreenDetectionTask;

impl TaskSpawner for FullscreenDetectionTask {
    fn spawn(self, app: &tauri::AppHandle) -> JoinHandle<()> {
        let app = app.clone();

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

                    let _ = app.typed_emit(
                        &FullscreenChangedPayload { is_fullscreen },
                    );
                    was_fullscreen = is_fullscreen;
                }

                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        })
    }
}

/// Windows 全屏检测实现
///
/// 通过 GetForegroundWindow 获取前台窗口，再用 GetWindowRect 获取窗口矩形，
/// 与所在显示器的屏幕尺寸比对，判断是否为全屏应用。
/// 排除桌面窗口（Shell_TrayWnd / Progman / WorkerW）避免误判。
#[cfg(target_os = "windows")]
fn check_fullscreen() -> bool {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetDesktopWindow, GetForegroundWindow, GetWindowRect,
    };

    unsafe {
        let fg_hwnd = GetForegroundWindow();
        if fg_hwnd == std::ptr::null_mut() || fg_hwnd == GetDesktopWindow() {
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
        if monitor == std::ptr::null_mut() {
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