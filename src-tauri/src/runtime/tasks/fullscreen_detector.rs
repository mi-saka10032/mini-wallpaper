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

use super::TaskSpawner;
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
/// 通过 `monitor_geometry` 提供的安全包装组合判定：
/// 1. 取前台窗口（`foreground_window` 已过滤桌面窗口本身）
/// 2. 排除系统桌面相关窗口类（Shell_TrayWnd / Progman / WorkerW）避免误判
/// 3. 比对窗口矩形与所在显示器矩形，完全覆盖即视为全屏
#[cfg(target_os = "windows")]
fn check_fullscreen() -> bool {
    use crate::platform::windows::monitor_geometry::{
        foreground_window, is_shell_window, monitor_info_from_window, window_rect,
    };

    let Some(hwnd) = foreground_window() else {
        return false;
    };

    // 排除系统桌面相关窗口类
    if is_shell_window(hwnd) {
        return false;
    }

    // 获取前台窗口矩形
    let Some(win_rect) = window_rect(hwnd) else {
        return false;
    };

    // 获取窗口所在显示器的信息
    let Some(monitor) = monitor_info_from_window(hwnd) else {
        return false;
    };

    // 窗口矩形覆盖整个显示器区域即视为全屏
    win_rect.covers_monitor(&monitor)
}

/// 非 Windows 平台：全屏检测不可用，始终返回 false
#[cfg(not(target_os = "windows"))]
fn check_fullscreen() -> bool {
    false
}