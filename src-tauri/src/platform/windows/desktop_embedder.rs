//! Win32 桌面壁纸嵌入模块
//!
//! 核心原理：
//! 1. FindWindow("Progman") 找到桌面窗口
//! 2. SendMessageTimeout(Progman, 0x052C) 触发 explorer 创建 WorkerW
//! 3. EnumWindows 找到正确的 WorkerW（包含 SHELLDLL_DefView 子窗口的那个的下一个兄弟）
//! 4. SetParent(tauri_hwnd, workerw) 将壁纸窗口嵌入桌面层级

#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT};
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
            ClientToScreen, GetMonitorInfoW, MonitorFromWindow, MONITORINFO,
            MONITOR_DEFAULTTOPRIMARY, ScreenToClient,
        },
        UI::WindowsAndMessaging::{
            GetClientRect, GetWindowLongPtrW, GetWindowRect, MoveWindow, SetWindowLongPtrW,
            SetWindowPos, GWL_EXSTYLE, GWL_STYLE, HWND_BOTTOM, SWP_FRAMECHANGED, SWP_NOACTIVATE,
            SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_BORDER, WS_CAPTION, WS_CHILD,
            WS_DLGFRAME, WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME, WS_EX_LAYERED,
            WS_EX_STATICEDGE, WS_EX_TRANSPARENT, WS_EX_WINDOWEDGE, WS_THICKFRAME, WS_VISIBLE,
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
        // let workerw = find_workerw()?;

        // 记录 SetParent 之前的窗口位置和尺寸
        let mut rect_before: RECT = zeroed();
        GetWindowRect(hwnd as HWND, &mut rect_before);
        let orig_x = rect_before.left;
        let orig_y = rect_before.top;
        let orig_w = rect_before.right - rect_before.left;
        let orig_h = rect_before.bottom - rect_before.top;

        println!(
            "[DesktopEmbedder] Before SetParent: pos=({}, {}), size={}x{}",
            orig_x, orig_y, orig_w, orig_h
        );

        // 4. SetParent 嵌入
        let prev_parent = SetParent(hwnd as HWND, workerw);
        if prev_parent == std::ptr::null_mut() {
            return Err("SetParent failed".into());
        }

        // ===== 5. 修复 24H2 非客户区偏移问题 =====
        //
        // Windows 11 24H2 在 SetParent 后会重新触发 WM_NCCALCSIZE，
        // 可能给无边框子窗口注入隐藏边框（~8px），导致：
        // - X 轴：壁纸内容整体右移
        // - Y 轴：顶部出现白边间隙
        //
        // 修复策略：清除所有边框样式 → SWP_FRAMECHANGED 强制重算 NC 区域 → 坐标补偿

        // 5a. 清除窗口样式中可能被注入的边框位
        let style = GetWindowLongPtrW(hwnd as HWND, GWL_STYLE);
        let clean_style = (style
            & !(WS_CAPTION as isize)
            & !(WS_THICKFRAME as isize)
            & !(WS_BORDER as isize)
            & !(WS_DLGFRAME as isize))
            | WS_CHILD as isize
            | WS_VISIBLE as isize;
        SetWindowLongPtrW(hwnd as HWND, GWL_STYLE, clean_style);

        // 5b. 清除扩展样式中的边框位，并设置鼠标事件穿透
        //
        // WS_EX_LAYERED + WS_EX_TRANSPARENT 组合效果：
        // - WS_EX_LAYERED：将窗口标记为分层窗口，启用 alpha 混合能力
        // - WS_EX_TRANSPARENT：让窗口在命中测试（hit-test）中被跳过，
        //   鼠标点击会穿透到 Z-order 下方的窗口（即桌面图标层 SHELLDLL_DefView）
        //
        // 这样用户在桌面上的所有鼠标操作（左键点击图标、右键弹出桌面菜单、
        // 拖拽选择等）都会直接作用于桌面，而非被 WebView 拦截
        let ex_style = GetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE);
        let clean_ex = (ex_style
            & !(WS_EX_CLIENTEDGE as isize)
            & !(WS_EX_STATICEDGE as isize)
            & !(WS_EX_WINDOWEDGE as isize)
            & !(WS_EX_DLGMODALFRAME as isize))
            | WS_EX_LAYERED as isize
            | WS_EX_TRANSPARENT as isize;
        SetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE, clean_ex);

        // 5c. SWP_FRAMECHANGED 强制系统重新发送 WM_NCCALCSIZE，
        //     在边框样式已清除的情况下，NC 区域会被重算为 0
        SetWindowPos(
            hwnd as HWND,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE,
        );

        // 5d. 将原始屏幕坐标转换为父窗口（WorkerW）客户区坐标
        //     这同时修复 24H2 WorkerW 坐标系原点可能不在 (0,0) 的问题
        let mut target_pt = POINT {
            x: orig_x,
            y: orig_y,
        };
        ScreenToClient(workerw, &mut target_pt);

        // 5e. 检查是否还有残余的非客户区偏移（双重保险）
        let mut client_origin = POINT { x: 0, y: 0 };
        ClientToScreen(hwnd as HWND, &mut client_origin);
        let mut window_rect_after: RECT = zeroed();
        GetWindowRect(hwnd as HWND, &mut window_rect_after);
        let nc_offset_x = client_origin.x - window_rect_after.left;
        let nc_offset_y = client_origin.y - window_rect_after.top;

        // 最终坐标 = 父窗口客户区坐标 - 残余 NC 偏移
        let final_x = target_pt.x - nc_offset_x;
        let final_y = target_pt.y - nc_offset_y;
        // 尺寸需要补偿两侧的 NC 区域
        let final_w = orig_w + nc_offset_x * 2;
        let final_h = orig_h + nc_offset_y * 2;

        println!(
            "[DesktopEmbedder] NC offset: ({}, {}), final pos: ({}, {}), final size: {}x{}",
            nc_offset_x, nc_offset_y, final_x, final_y, final_w, final_h
        );

        MoveWindow(hwnd as HWND, final_x, final_y, final_w, final_h, 1);

        println!(
            "[DesktopEmbedder] Embedded HWND {:?} into WorkerW {:?} (with NC fix)",
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
