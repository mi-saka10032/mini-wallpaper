//! Win32 桌面壁纸嵌入模块
//!
//! 核心原理：
//! 1. FindWindow("Progman") 找到桌面窗口
//! 2. SendMessageTimeout(Progman, 0x052C) 触发 explorer 创建 WorkerW
//! 3. EnumWindows 找到正确的 WorkerW（包含 SHELLDLL_DefView 子窗口的那个的下一个兄弟）
//! 4. SetParent(tauri_hwnd, workerw) 将壁纸窗口嵌入桌面层级

#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, SendMessageTimeoutW, SetParent, SMTO_NORMAL,
};

#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicIsize, Ordering};

use std::mem::size_of;

/// 在 Windows 上查找 WorkerW 窗口并将指定 HWND 嵌入桌面层级
#[cfg(target_os = "windows")]
pub fn embed_in_desktop(hwnd: isize) -> Result<(), String> {
    use std::mem::zeroed;

    use windows_sys::Win32::{
        Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
        },
        UI::WindowsAndMessaging::{
            GetClientRect, GetWindowLongPtrW, GetWindowRect, MoveWindow, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_BOTTOM, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_EX_LAYERED, WS_EX_TRANSPARENT
        },
    };

    unsafe {
        // 1. 找到 Progman 窗口
        let progman = FindWindowW(encode_wide("Progman\0").as_ptr(), std::ptr::null());
        if progman == std::ptr::null_mut() {
            return Err("Failed to find Progman window".into());
        }

        // 2. 发送 0x052C 消息触发 WorkerW 创建
        let mut _result: usize = 0;
        SendMessageTimeoutW(
            progman,
            0x052C,
            0,
            0,
            SMTO_NORMAL,
            1000,
            &mut _result as *mut usize,
        );

        // 找到 Progman 的第一个子窗口（就是壁纸层 WorkerW）
        let workerw = FindWindowExW(
            progman,
            std::ptr::null_mut(),
            encode_wide("WorkerW\0").as_ptr(),
            std::ptr::null(),
        );

        // 3. 枚举所有顶层窗口，找到目标 WorkerW
        // let workerw = find_workerw()?;1

        // 4. SetParent 嵌入
        let prev_parent = SetParent(hwnd as HWND, workerw);
        if prev_parent == std::ptr::null_mut() {
            return Err("SetParent failed".into());
        }

        println!(
            "[DesktopEmbedder] Embedded HWND {:?} into WorkerW {:?}",
            hwnd, workerw
        );
        Ok(())
    }
}

/// 枚举顶层窗口，找到桌面 WorkerW
///
/// 逻辑：找到包含 "SHELLDLL_DefView" 子窗口的 WorkerW，
/// 然后取它在 Z-order 上的下一个 WorkerW 兄弟窗口（那才是壁纸应该嵌入的目标）
#[cfg(target_os = "windows")]
unsafe fn find_workerw() -> Result<HWND, String> {
    static FOUND_WORKERW: AtomicIsize = AtomicIsize::new(0);
    FOUND_WORKERW.store(0, Ordering::SeqCst);

    unsafe extern "system" fn enum_callback(hwnd: HWND, _lparam: LPARAM) -> BOOL {
        let shell_view = FindWindowExW(
            hwnd,
            std::ptr::null_mut(),
            encode_wide("SHELLDLL_DefView\0").as_ptr(),
            std::ptr::null(),
        );

        // 找到包含 SHELLDLL_DefView 的 WorkerW 后
        if shell_view != std::ptr::null_mut() {
            // 需要找它的上一个 WorkerW
            // 由于 FindWindowExW 只能往后找，重新枚举所有 WorkerW 找到 hwnd 的前一个
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
                    // 找到了目标，prev_workerw 就是它的上一个
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

    EnumWindows(Some(enum_callback), 0);

    let result = FOUND_WORKERW.load(Ordering::SeqCst);
    if result == 0 {
        Err("Failed to find WorkerW window".into())
    } else {
        Ok(result as HWND)
    }
}

/// 从桌面层级中移除嵌入的窗口（还原 parent 为桌面）
#[cfg(target_os = "windows")]
pub fn unembed_from_desktop(hwnd: isize) {
    unsafe {
        // SetParent(hwnd, NULL) 将窗口还原为顶层窗口
        SetParent(hwnd as HWND, std::ptr::null_mut());
        println!("[DesktopEmbedder] Unembedded HWND {:?}", hwnd);
    }
}

/// 辅助函数：将 &str 编码为以 null 结尾的 UTF-16 Vec
#[cfg(target_os = "windows")]
fn encode_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

// ===== 非 Windows 平台的空实现 =====

#[cfg(not(target_os = "windows"))]
pub fn embed_in_desktop(_hwnd: isize) -> Result<(), String> {
    println!("[DesktopEmbedder] embed_in_desktop is a no-op on this platform");
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn unembed_from_desktop(_hwnd: isize) {
    println!("[DesktopEmbedder] unembed_from_desktop is a no-op on this platform");
}
