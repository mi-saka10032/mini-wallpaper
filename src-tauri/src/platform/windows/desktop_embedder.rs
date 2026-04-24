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
            SetWindowPos, GWL_EXSTYLE, GWL_STYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE,
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

        // 使用前端传入的显示器坐标和尺寸（单个显示器的物理分辨率）
        // 而非 WorkerW 客户区（覆盖整个虚拟桌面，多显示器下会合并）
        let target_x = monitor_x;
        let target_y = monitor_y;
        let target_w = monitor_width;
        let target_h = monitor_height;

        println!(
            "[DesktopEmbedder] Monitor rect: ({}, {}) {}x{}",
            target_x, target_y, target_w, target_h
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
        // 修复策略：
        // 1. 清除所有边框样式 → SWP_FRAMECHANGED 强制重算 NC 区域
        // 2. 以前端传入的显示器物理分辨率为目标尺寸（避免 WorkerW 客户区合并多显示器）
        // 3. 精确测量四边 NC 偏移（不假设对称），反向补偿
        // 4. 测量→补偿→验证 闭环

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

        // 5d. 先将窗口移到 WorkerW 中该显示器对应的位置，尺寸设为目标尺寸
        //     注意：WorkerW 覆盖整个虚拟桌面，所以需要用显示器在虚拟桌面中的坐标定位
        //     这是初始定位，后续会根据实际测量结果进行补偿
        MoveWindow(hwnd as HWND, target_x, target_y, target_w, target_h, 1);

        // 5e. 测量→补偿→验证 闭环（最多 3 轮）
        //
        // 原理：MoveWindow 设置的是窗口矩形（包含 NC 区域），但我们需要的是
        // 客户区完全覆盖该显示器区域。如果存在 NC 偏移，客户区会比窗口矩形小，
        // 导致右侧/底部出现缺口。
        //
        // 通过测量 "窗口矩形 vs 客户区矩形" 的差值，精确计算四边 NC 尺寸，
        // 然后扩大窗口矩形并偏移位置来补偿。
        let mut final_x = target_x;
        let mut final_y = target_y;
        let mut final_w = target_w;
        let mut final_h = target_h;

        for round in 0..3 {
            // 测量当前窗口矩形和客户区矩形
            let mut win_rect: RECT = zeroed();
            GetWindowRect(hwnd as HWND, &mut win_rect);

            let mut client_rect: RECT = zeroed();
            GetClientRect(hwnd as HWND, &mut client_rect);

            // 客户区左上角在屏幕上的坐标
            let mut client_origin = POINT { x: 0, y: 0 };
            ClientToScreen(hwnd as HWND, &mut client_origin);

            // 四边 NC 尺寸（精确测量，不假设对称）
            let nc_left = client_origin.x - win_rect.left;
            let nc_top = client_origin.y - win_rect.top;
            let nc_right = win_rect.right - (client_origin.x + client_rect.right);
            let nc_bottom = win_rect.bottom - (client_origin.y + client_rect.bottom);

            let client_w = client_rect.right - client_rect.left;
            let client_h = client_rect.bottom - client_rect.top;

            println!(
                "[DesktopEmbedder] Round {}: NC edges L={} T={} R={} B={}, client={}x{}, target={}x{}",
                round, nc_left, nc_top, nc_right, nc_bottom, client_w, client_h, target_w, target_h
            );

            // 如果客户区已经完全覆盖目标区域，补偿完成
            if nc_left == 0 && nc_top == 0 && nc_right == 0 && nc_bottom == 0
                && client_w == target_w && client_h == target_h
            {
                println!("[DesktopEmbedder] Round {}: No NC offset, perfect fit!", round);
                break;
            }

            // 计算补偿：窗口位置向左上偏移 NC 尺寸，窗口大小扩大 NC 总量
            final_x = target_x - nc_left;
            final_y = target_y - nc_top;
            final_w = target_w + nc_left + nc_right;
            final_h = target_h + nc_top + nc_bottom;

            println!(
                "[DesktopEmbedder] Round {}: Compensating → pos=({}, {}), size={}x{}",
                round, final_x, final_y, final_w, final_h
            );

            MoveWindow(hwnd as HWND, final_x, final_y, final_w, final_h, 1);
        }

        println!(
            "[DesktopEmbedder] Embedded HWND {:?} into WorkerW {:?} (final: pos=({},{}), size={}x{})",
            hwnd, workerw, final_x, final_y, final_w, final_h
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