//! 经典 WorkerW 查找（EnumWindows 方案）
//!
//! 通过 EnumWindows 遍历所有顶层窗口，找到包含 SHELLDLL_DefView 的 WorkerW，
//! 取其在 Z-order 上的前一个 WorkerW 兄弟窗口。
//!
//! 此方案适用于所有 Windows 版本，也作为 24H2+ 路径的 fallback。

use anyhow::{bail, Result};
use std::sync::atomic::{AtomicIsize, Ordering};
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, FindWindowExW};

use super::encode_wide;

/// 通过 EnumWindows 查找桌面 WorkerW（经典方案）
pub(super) fn find_workerw_classic() -> Result<HWND> {
    static FOUND_WORKERW: AtomicIsize = AtomicIsize::new(0);
    FOUND_WORKERW.store(0, Ordering::SeqCst);

    unsafe extern "system" fn enum_callback(hwnd: HWND, _lparam: LPARAM) -> BOOL {
        let shell_view = FindWindowExW(
            hwnd,
            std::ptr::null_mut(),
            encode_wide("SHELLDLL_DefView\0").as_ptr(),
            std::ptr::null(),
        );

        if shell_view != std::ptr::null_mut() {
            let mut prev_workerw = std::ptr::null_mut();
            let mut current = std::ptr::null_mut();

            loop {
                current = FindWindowExW(
                    std::ptr::null_mut(),
                    current,
                    encode_wide("WorkerW\0").as_ptr(),
                    std::ptr::null(),
                );
                if current == std::ptr::null_mut() {
                    break;
                }
                if current == hwnd {
                    if prev_workerw != std::ptr::null_mut() {
                        FOUND_WORKERW.store(prev_workerw as isize, Ordering::SeqCst);
                    }
                    break;
                }
                prev_workerw = current;
            }
        }
        1 // TRUE = 继续枚举
    }

    unsafe { EnumWindows(Some(enum_callback), 0) };

    let result = FOUND_WORKERW.load(Ordering::SeqCst);
    if result == 0 {
        bail!("经典方案未找到 WorkerW 窗口");
    }
    Ok(result as HWND)
}
