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

/// 在 Windows 上查找 WorkerW 窗口并将指定 HWND 嵌入桌面层级
///
/// 参数：
/// - `hwnd`: 要嵌入的窗口句柄
/// - `monitor_x`, `monitor_y`: 该显示器在虚拟桌面中的左上角坐标
/// - `monitor_width`, `monitor_height`: 该显示器的物理分辨率
#[cfg(target_os = "windows")]
pub fn embed_in_desktop(hwnd: isize, monitor_x: i32, monitor_y: i32, monitor_width: i32, monitor_height: i32) -> Result<(), String> {
    use std::mem::zeroed;

    use windows_sys::Win32::{
        Graphics::Gdi::{
            ClientToScreen,
        },
        UI::WindowsAndMessaging::{
            GetClientRect, GetWindowLongPtrW, GetWindowRect, MoveWindow, SetWindowLongPtrW,
            SetWindowPos, SetLayeredWindowAttributes, GWL_EXSTYLE, GWL_STYLE, LWA_ALPHA,
            SWP_FRAMECHANGED, SWP_NOACTIVATE,
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

        // 使用前端传入的显示器坐标和尺寸（单个显示器的物理分辨率）
        let target_x = monitor_x;
        let target_y = monitor_y;
        let target_w = monitor_width;
        let target_h = monitor_height;

        println!(
            "[DesktopEmbedder] Monitor rect: ({}, {}) {}x{}",
            target_x, target_y, target_w, target_h
        );

        // ===== 3. 方案 B：在 SetParent 之前设置 WS_EX_LAYERED =====
        //
        // Windows 11 24H2 改变了 DWM 桌面合成机制，要求嵌入桌面的窗口
        // 必须是 Layered 窗口才能正确参与合成。如果在 SetParent 之后才设置
        // WS_EX_LAYERED，DWM 已经按非 Layered 窗口的方式处理了 NC 区域，
        // 会注入隐藏边框（~8px 偏移）。
        //
        // 方案 B 的核心思路：
        // 1. 在 SetParent 之前就将窗口标记为 Layered 并清除所有边框样式
        // 2. 调用 SetLayeredWindowAttributes 设置完全不透明（bAlpha=0xFF）
        // 3. 这样 DWM 在 SetParent 触发的 WM_NCCALCSIZE 中就不会注入 NC 边框
        // 4. 从根源消除偏移问题，无需事后补偿

        // 3a. 清除窗口样式中的所有边框位
        let style = GetWindowLongPtrW(hwnd as HWND, GWL_STYLE);
        let clean_style = (style
            & !(WS_CAPTION as isize)
            & !(WS_THICKFRAME as isize)
            & !(WS_BORDER as isize)
            & !(WS_DLGFRAME as isize))
            | WS_CHILD as isize
            | WS_VISIBLE as isize;
        SetWindowLongPtrW(hwnd as HWND, GWL_STYLE, clean_style);

        // 3b. 设置扩展样式：WS_EX_LAYERED + WS_EX_TRANSPARENT（鼠标穿透）
        //     清除所有可能的边框扩展样式
        //
        // WS_EX_LAYERED：将窗口标记为分层窗口，启用 alpha 混合能力
        //   - 24H2 要求壁纸窗口必须是 Layered 才能正确参与 DWM 合成
        // WS_EX_TRANSPARENT：让窗口在命中测试（hit-test）中被跳过，
        //   鼠标点击会穿透到 Z-order 下方的窗口（即桌面图标层 SHELLDLL_DefView）
        let ex_style = GetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE);
        let clean_ex = (ex_style
            & !(WS_EX_CLIENTEDGE as isize)
            & !(WS_EX_STATICEDGE as isize)
            & !(WS_EX_WINDOWEDGE as isize)
            & !(WS_EX_DLGMODALFRAME as isize))
            | WS_EX_LAYERED as isize
            | WS_EX_TRANSPARENT as isize;
        SetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE, clean_ex);

        // 3c. 设置 Layered 窗口属性：完全不透明（bAlpha=0xFF）
        //     这是 24H2 下 Layered 窗口正确显示的关键调用
        //     如果不调用此函数，Layered 窗口默认是完全透明（不可见）的
        SetLayeredWindowAttributes(hwnd as HWND, 0, 0xFF, LWA_ALPHA);

        // 3d. SWP_FRAMECHANGED 强制系统重新发送 WM_NCCALCSIZE，
        //     在边框样式已清除 + Layered 已设置的情况下，NC 区域应被重算为 0
        SetWindowPos(
            hwnd as HWND,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE,
        );

        println!("[DesktopEmbedder] Pre-SetParent: WS_EX_LAYERED set, SetLayeredWindowAttributes(0xFF) called");

        // ===== 4. SetParent 嵌入 =====
        let prev_parent = SetParent(hwnd as HWND, workerw);
        if prev_parent == std::ptr::null_mut() {
            return Err("SetParent failed".into());
        }

        // 4a. SetParent 后再次 SWP_FRAMECHANGED，确保 NC 区域在新父窗口下也正确
        SetWindowPos(
            hwnd as HWND,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE,
        );

        // ===== 5. 定位窗口到目标显示器区域 =====
        MoveWindow(hwnd as HWND, target_x, target_y, target_w, target_h, 1);

        // ===== 6. 验证：测量实际 NC 偏移，输出日志 =====
        //     如果方案 B 生效，NC 偏移应该全部为 0
        let mut win_rect: RECT = zeroed();
        GetWindowRect(hwnd as HWND, &mut win_rect);

        let mut client_rect: RECT = zeroed();
        GetClientRect(hwnd as HWND, &mut client_rect);

        let mut client_origin = POINT { x: 0, y: 0 };
        ClientToScreen(hwnd as HWND, &mut client_origin);

        let nc_left = client_origin.x - win_rect.left;
        let nc_top = client_origin.y - win_rect.top;
        let nc_right = win_rect.right - (client_origin.x + client_rect.right);
        let nc_bottom = win_rect.bottom - (client_origin.y + client_rect.bottom);

        let client_w = client_rect.right - client_rect.left;
        let client_h = client_rect.bottom - client_rect.top;

        println!(
            "[DesktopEmbedder] Verify: NC edges L={} T={} R={} B={}, client={}x{}, target={}x{}",
            nc_left, nc_top, nc_right, nc_bottom, client_w, client_h, target_w, target_h
        );

        if nc_left != 0 || nc_top != 0 || nc_right != 0 || nc_bottom != 0 {
            println!(
                "[DesktopEmbedder] WARNING: NC offset still present after Plan B! L={} T={} R={} B={}",
                nc_left, nc_top, nc_right, nc_bottom
            );
            println!(
                "[DesktopEmbedder] Falling back to NC compensation...",
            );
            // 回退：如果 Layered 前置仍未消除 NC，则进行一次补偿
            let comp_x = target_x - nc_left;
            let comp_y = target_y - nc_top;
            let comp_w = target_w + nc_left + nc_right;
            let comp_h = target_h + nc_top + nc_bottom;
            MoveWindow(hwnd as HWND, comp_x, comp_y, comp_w, comp_h, 1);
            println!(
                "[DesktopEmbedder] Compensated → pos=({}, {}), size={}x{}",
                comp_x, comp_y, comp_w, comp_h
            );
        } else {
            println!("[DesktopEmbedder] Plan B success: No NC offset detected!");
        }

        println!(
            "[DesktopEmbedder] Embedded HWND {:?} into WorkerW {:?} (pos=({},{}), size={}x{})",
            hwnd, workerw, target_x, target_y, target_w, target_h
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
pub fn embed_in_desktop(_hwnd: isize, _monitor_x: i32, _monitor_y: i32, _monitor_width: i32, _monitor_height: i32) -> Result<(), String> {
    println!("[DesktopEmbedder] embed_in_desktop is a no-op on this platform");
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn unembed_from_desktop(_hwnd: isize) {
    println!("[DesktopEmbedder] unembed_from_desktop is a no-op on this platform");
}