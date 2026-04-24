//! Win32 桌面壁纸嵌入模块 — Direct Child 方案
//!
//! 基于博主的 Direct Child 方案实现，核心原理：
//!
//! 1. FindWindow("Progman") 找到桌面窗口
//! 2. RaiseDesktop: 发送多次 0x052C 消息确保壁纸层级就绪
//! 3. 查找 SHELLDLL_DefView 的位置：
//!    - 优先在 Progman 中查找（24H2/25H2 常见）
//!    - 回退：遍历所有顶级 WorkerW 窗口查找（23H2 及其他情况）
//! 4. Z-order 管理：壁纸 → HWND_TOP, DefView → HWND_TOP, WorkerW → HWND_BOTTOM
//!    确保层级为：DefView(图标) > 壁纸窗口 > WorkerW(系统壁纸)
//! 5. 定时器监控：确保壁纸始终紧跟在 DefView 正下方
//!
//! 关键技巧：
//! - 使用 WS_EX_LAYERED + SetLayeredWindowAttributes 实现透明
//! - 使用 DwmExtendFrameIntoClientArea + DwmEnableBlurBehindWindow 实现背景透明
//! - 不隐藏任何 WorkerW，通过 Z-order 管理层级

#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, GetWindow, IsWindowVisible,
    MoveWindow, SendMessageTimeoutW, SetParent, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, SMTO_NORMAL,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Graphics::Gdi::CreateRectRgn;

#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
#[cfg(target_os = "windows")]
use std::sync::Mutex;

// ===== DWM API 原始 FFI 声明 =====

#[cfg(target_os = "windows")]
#[repr(C)]
struct MARGINS {
    cxLeftWidth: i32,
    cxRightWidth: i32,
    cyTopHeight: i32,
    cyBottomHeight: i32,
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct DWM_BLURBEHIND {
    dwFlags: u32,
    fEnable: i32,
    hRgnBlur: windows_sys::Win32::Graphics::Gdi::HRGN,
    fTransitionOnMaximized: i32,
}

#[cfg(target_os = "windows")]
#[link(name = "dwmapi")]
extern "system" {
    fn DwmExtendFrameIntoClientArea(hwnd: HWND, pMarInset: *const MARGINS) -> i32;
    fn DwmEnableBlurBehindWindow(hwnd: HWND, pBlurBehind: *const DWM_BLURBEHIND) -> i32;
    fn DwmSetWindowAttribute(hwnd: HWND, dwAttribute: u32, pvAttribute: *const u32, cbAttribute: u32) -> i32;
}

// ===== 全局状态 =====

#[cfg(target_os = "windows")]
static EMBEDDED_WINDOWS: Mutex<Option<Vec<EmbeddedWindow>>> = Mutex::new(None);

#[cfg(target_os = "windows")]
static ZORDER_TIMER_RUNNING: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "windows")]
static CACHED_DEFVIEW: AtomicIsize = AtomicIsize::new(0);

#[cfg(target_os = "windows")]
static CACHED_EMBED_TARGET: AtomicIsize = AtomicIsize::new(0);

#[cfg(target_os = "windows")]
static CACHED_WORKERW: AtomicIsize = AtomicIsize::new(0);

#[cfg(target_os = "windows")]
static IS_LEGACY_MODE: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "windows")]
#[derive(Clone, Debug)]
struct EmbeddedWindow {
    hwnd: isize,
    monitor_x: i32,
    monitor_y: i32,
    monitor_w: i32,
    monitor_h: i32,
}

// ===== 辅助函数 =====

/// 将 &str 编码为以 null 结尾的 UTF-16 Vec
#[cfg(target_os = "windows")]
fn encode_wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// 获取窗口类名
#[cfg(target_os = "windows")]
unsafe fn get_class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let len = GetClassNameW(hwnd, buf.as_mut_ptr(), 256);
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..len as usize])
}

/// 检查窗口是否为 WorkerW 类
#[cfg(target_os = "windows")]
unsafe fn is_worker_class(hwnd: HWND) -> bool {
    if hwnd == std::ptr::null_mut() {
        return false;
    }
    let name = get_class_name(hwnd);
    name == "WorkerW" || name == "WorkerA"
}

/// 发送 0x052C 系列消息，确保桌面壁纸层级就绪
///
/// 参考 ExplorerPatcher: https://github.com/valinet/ExplorerPatcher/issues/525
#[cfg(target_os = "windows")]
unsafe fn raise_desktop(progman: HWND) -> bool {
    let mut res0: usize = usize::MAX;
    let mut res1: usize = usize::MAX;
    let mut res2: usize = usize::MAX;
    let mut res3: usize = usize::MAX;

    SendMessageTimeoutW(
        progman, 0x052C, 0xA, 0, SMTO_NORMAL, 1000,
        &mut res0 as *mut usize,
    );
    if res0 != 0 {
        println!("[DesktopEmbedder] RaiseDesktop: wallpaper not initialized (res0={})", res0);
    }

    SendMessageTimeoutW(
        progman, 0x052C, 0xD, 0, SMTO_NORMAL, 1000,
        &mut res1 as *mut usize,
    );
    SendMessageTimeoutW(
        progman, 0x052C, 0xD, 1, SMTO_NORMAL, 1000,
        &mut res2 as *mut usize,
    );
    SendMessageTimeoutW(
        progman, 0x052C, 0, 0, SMTO_NORMAL, 1000,
        &mut res3 as *mut usize,
    );

    let success = res1 == 0 && res2 == 0 && res3 == 0;
    println!(
        "[DesktopEmbedder] RaiseDesktop: res0={}, res1={}, res2={}, res3={}, success={}",
        res0, res1, res2, res3, success
    );
    success
}

/// 在所有顶级窗口中查找 SHELLDLL_DefView
///
/// 25H2/24H2: DefView 可能在 Progman 中，也可能在某个 WorkerW 中
/// 此函数先查 Progman，再遍历所有顶级 WorkerW 窗口
///
/// 返回 (defview_hwnd, defview_parent_hwnd)
#[cfg(target_os = "windows")]
unsafe fn find_defview_globally(progman: HWND) -> (HWND, HWND) {
    let defview_class = encode_wide("SHELLDLL_DefView");

    // 首先在 Progman 中查找
    let defview = FindWindowExW(
        progman,
        std::ptr::null_mut(),
        defview_class.as_ptr(),
        std::ptr::null(),
    );
    if defview != std::ptr::null_mut() {
        println!(
            "[DesktopEmbedder] DefView 0x{:X} found in Progman 0x{:X}",
            defview as isize, progman as isize
        );
        return (defview, progman);
    }

    // Progman 中没有，遍历所有顶级窗口查找
    println!("[DesktopEmbedder] DefView not in Progman, enumerating top-level windows...");

    #[repr(C)]
    struct FindResult {
        defview: HWND,
        parent: HWND,
    }
    let mut result = FindResult {
        defview: std::ptr::null_mut(),
        parent: std::ptr::null_mut(),
    };

    unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: isize) -> i32 {
        let result = &mut *(lparam as *mut FindResult);
        let class_name = get_class_name(hwnd);
        if class_name != "WorkerW" && class_name != "WorkerA" {
            return 1; // 继续枚举
        }

        let defview_class = encode_wide("SHELLDLL_DefView");
        let defview = FindWindowExW(
            hwnd,
            std::ptr::null_mut(),
            defview_class.as_ptr(),
            std::ptr::null(),
        );
        if defview != std::ptr::null_mut() {
            result.defview = defview;
            result.parent = hwnd;
            println!(
                "[DesktopEmbedder] DefView 0x{:X} found in WorkerW 0x{:X}",
                defview as isize, hwnd as isize
            );
            return 0; // 停止枚举
        }
        1 // 继续枚举
    }

    EnumWindows(
        Some(enum_callback),
        &mut result as *mut FindResult as isize,
    );

    (result.defview, result.parent)
}

/// 确保壁纸窗口在 DefView 正下方
///
/// 返回 (是否正常, 冲突窗口句柄)
#[cfg(target_os = "windows")]
unsafe fn ensure_embed_below_defview(defview: HWND, embed_wnd: HWND) -> (bool, HWND) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GW_HWNDPREV, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };

    let prev = GetWindow(embed_wnd, GW_HWNDPREV);

    // 层级已正确：壁纸窗口的上一个兄弟就是 DefView
    if prev == defview {
        return (true, std::ptr::null_mut());
    }

    // 上一个兄弟是 WorkerW 也视为正常（多窗口嵌入时可能出现）
    // 关键：不要调用 SetWindowPos，避免无谓的重绘导致闪烁
    if is_worker_class(prev) {
        return (true, std::ptr::null_mut());
    }

    // 真正需要修正 Z-order 的情况：被非 WorkerW 的窗口插入
    println!(
        "[DesktopEmbedder] Z-order fix needed: prev=0x{:X}, moving embed 0x{:X} after DefView 0x{:X}",
        prev as isize, embed_wnd as isize, defview as isize
    );
    SetWindowPos(
        embed_wnd, defview, 0, 0, 0, 0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
    );

    (false, prev)
}

/// 打印桌面窗口层级结构（诊断用）
#[cfg(target_os = "windows")]
unsafe fn dump_desktop_hierarchy(progman: HWND) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GW_CHILD, GW_HWNDNEXT};

    println!("[DesktopEmbedder] === Desktop Hierarchy Dump ===");
    println!(
        "[DesktopEmbedder] Progman: 0x{:X}, visible={}",
        progman as isize,
        IsWindowVisible(progman) != 0
    );

    // 枚举 Progman 的子窗口
    let mut child = GetWindow(progman, GW_CHILD);
    while child != std::ptr::null_mut() {
        let class = get_class_name(child);
        let visible = IsWindowVisible(child) != 0;
        println!(
            "[DesktopEmbedder]   Progman child: 0x{:X} class='{}' visible={}",
            child as isize, class, visible
        );
        if class == "SHELLDLL_DefView" {
            let mut sub = GetWindow(child, GW_CHILD);
            while sub != std::ptr::null_mut() {
                let sub_class = get_class_name(sub);
                println!(
                    "[DesktopEmbedder]     DefView child: 0x{:X} class='{}'",
                    sub as isize, sub_class
                );
                sub = GetWindow(sub, GW_HWNDNEXT);
            }
        }
        child = GetWindow(child, GW_HWNDNEXT);
    }

    // 枚举顶级 WorkerW 窗口
    unsafe extern "system" fn enum_workers(hwnd: HWND, _: isize) -> i32 {
        let class = get_class_name(hwnd);
        if class == "WorkerW" || class == "WorkerA" {
            use windows_sys::Win32::UI::WindowsAndMessaging::{GW_CHILD, GW_HWNDNEXT};
            let visible = IsWindowVisible(hwnd) != 0;
            println!(
                "[DesktopEmbedder]   Top-level {}: 0x{:X} visible={}",
                class, hwnd as isize, visible
            );
            let mut child = GetWindow(hwnd, GW_CHILD);
            while child != std::ptr::null_mut() {
                let child_class = get_class_name(child);
                println!(
                    "[DesktopEmbedder]     {} child: 0x{:X} class='{}'",
                    class, child as isize, child_class
                );
                child = GetWindow(child, GW_HWNDNEXT);
            }
        }
        1
    }
    EnumWindows(Some(enum_workers), 0);
    println!("[DesktopEmbedder] === End Hierarchy Dump ===");
}

// ===== 主要公共 API =====

/// 在 Windows 上将壁纸窗口嵌入桌面层级
///
/// 参数：
/// - `hwnd`: 要嵌入的窗口句柄
/// - `monitor_x`, `monitor_y`: 该显示器在虚拟桌面中的左上角坐标
/// - `monitor_width`, `monitor_height`: 该显示器的物理分辨率
#[cfg(target_os = "windows")]
pub fn embed_in_desktop(
    hwnd: isize,
    monitor_x: i32,
    monitor_y: i32,
    monitor_width: i32,
    monitor_height: i32,
) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetLayeredWindowAttributes, GWL_EXSTYLE, GWL_STYLE,
        HWND_BOTTOM, HWND_TOP, LWA_ALPHA,
        SWP_DRAWFRAME, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SW_SHOW,
        WS_BORDER, WS_CAPTION, WS_CHILD, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
        WS_OVERLAPPED, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
    };

    unsafe {
        println!(
            "[DesktopEmbedder] Monitor rect: ({}, {}) {}x{}",
            monitor_x, monitor_y, monitor_width, monitor_height
        );

        // ===== 1. 找到 Progman 窗口 =====
        let progman = FindWindowW(
            encode_wide("Progman").as_ptr(),
            encode_wide("Program Manager").as_ptr(),
        );
        if progman == std::ptr::null_mut() {
            return Err("Failed to find Progman window".into());
        }
        println!("[DesktopEmbedder] Found Progman: 0x{:X}", progman as isize);

        // ===== 2. RaiseDesktop =====
        if !raise_desktop(progman) {
            println!("[DesktopEmbedder] WARNING: RaiseDesktop returned false, continuing anyway...");
        }

        // 等待 explorer 处理消息
        std::thread::sleep(std::time::Duration::from_millis(200));

        // ===== 3. 打印桌面层级结构（诊断） =====
        dump_desktop_hierarchy(progman);

        // ===== 4. 查找 SHELLDLL_DefView =====
        let (defview, defview_parent) = find_defview_globally(progman);
        if defview == std::ptr::null_mut() {
            return Err("Failed to find SHELLDLL_DefView in any window".into());
        }

        // 判断模式：DefView 在 Progman 中 = 24H2/25H2 模式，否则 = legacy 模式
        let is_legacy = defview_parent != progman;
        IS_LEGACY_MODE.store(is_legacy, Ordering::SeqCst);
        CACHED_DEFVIEW.store(defview as isize, Ordering::SeqCst);

        println!(
            "[DesktopEmbedder] Mode: {}, DefView=0x{:X}, DefView parent=0x{:X}",
            if is_legacy { "legacy (DefView in WorkerW)" } else { "modern (DefView in Progman)" },
            defview as isize,
            defview_parent as isize
        );

        // ===== 5. 确定嵌入目标和 WorkerW =====
        //
        // 博主方案核心逻辑：
        // - 24H2/25H2 (DefView 在 Progman): SetParent 到 Progman
        // - legacy (DefView 在 WorkerW): SetParent 到该 WorkerW 或 Progman
        //
        // WorkerW 查找：Progman 内部的 WorkerW 是系统壁纸窗口
        let embed_target = if is_legacy {
            // legacy 模式：嵌入到 DefView 所在的 WorkerW（或 Progman）
            defview_parent
        } else {
            // modern 模式：嵌入到 Progman
            progman
        };

        // 找到 Progman 内部的 WorkerW（系统壁纸窗口，需要沉底）
        let mut worker_in_progman = FindWindowExW(
            progman,
            std::ptr::null_mut(),
            encode_wide("WorkerW").as_ptr(),
            encode_wide("").as_ptr(),
        );
        if worker_in_progman == std::ptr::null_mut() {
            worker_in_progman = FindWindowExW(
                progman,
                std::ptr::null_mut(),
                encode_wide("WorkerA").as_ptr(),
                encode_wide("").as_ptr(),
            );
        }

        // 如果 Progman 内没有 WorkerW，用 Progman 自身作为 fallback
        let effective_worker = if worker_in_progman != std::ptr::null_mut() {
            worker_in_progman
        } else {
            println!("[DesktopEmbedder] WARNING: No WorkerW found inside Progman");
            progman
        };

        CACHED_EMBED_TARGET.store(embed_target as isize, Ordering::SeqCst);
        CACHED_WORKERW.store(effective_worker as isize, Ordering::SeqCst);

        println!(
            "[DesktopEmbedder] Embed target: 0x{:X}, WorkerW: 0x{:X}",
            embed_target as isize, effective_worker as isize
        );

        // ===== 6. 准备窗口样式（参照博主代码） =====
        let mut style = GetWindowLongPtrW(hwnd as HWND, GWL_STYLE);
        let mut ex_style = GetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE);

        // 添加 WS_EX_LAYERED
        ex_style |= WS_EX_LAYERED as isize;
        // 移除 WS_EX_TOOLWINDOW
        ex_style &= !(WS_EX_TOOLWINDOW as isize);
        // 移除不需要的样式（与博主代码一致）
        style &= !(WS_CHILD as isize);
        style &= !(WS_POPUP as isize);
        style &= !(WS_OVERLAPPED as isize);
        style &= !(WS_CAPTION as isize);
        style &= !(WS_BORDER as isize);
        style &= !(WS_SYSMENU as isize);
        style &= !(WS_THICKFRAME as isize);

        SetWindowLongPtrW(hwnd as HWND, GWL_STYLE, style);
        SetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE, ex_style);
        println!("[DesktopEmbedder] Styles set: style=0x{:X}, ex_style=0x{:X}", style, ex_style);

        // ===== 6.5 禁用 DWM 非客户区渲染，消除 NC offset =====
        {
            // DWMWA_NCRENDERING_POLICY = 2, DWMNCRP_DISABLED = 1
            let policy: u32 = 1; // DWMNCRP_DISABLED
            let hr = DwmSetWindowAttribute(
                hwnd as HWND,
                2, // DWMWA_NCRENDERING_POLICY
                &policy as *const u32,
                std::mem::size_of::<u32>() as u32,
            );
            println!("[DesktopEmbedder] DWM NCRENDERING_POLICY set to DISABLED (hr=0x{:X})", hr);
        }

        // 通知系统重新计算非客户区（使样式变更和 DWM 策略生效）
        SetWindowPos(
            hwnd as HWND,
            std::ptr::null_mut(),
            0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_DRAWFRAME | 0x0020, // SWP_FRAMECHANGED = SWP_DRAWFRAME, SWP_NOZORDER = 0x0004 不需要
        );

        // ===== 7. DWM 透明 hack（参照博主代码） =====
        // 关键：先 SetParent(NULL) 确保是顶级窗口，DWM 才允许透明
        SetParent(hwnd as HWND, std::ptr::null_mut());

        // DwmExtendFrameIntoClientArea
        {
            let margins = MARGINS {
                cxLeftWidth: 0,
                cxRightWidth: 0,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            };
            let hr = DwmExtendFrameIntoClientArea(hwnd as HWND, &margins);
            println!("[DesktopEmbedder] DwmExtendFrameIntoClientArea: hr=0x{:X}", hr);
        }

        // DwmEnableBlurBehindWindow
        {
            let h_rgn = CreateRectRgn(0, 0, -1, -1);
            let bb = DWM_BLURBEHIND {
                dwFlags: 0x1 | 0x2, // DWM_BB_ENABLE | DWM_BB_BLURREGION
                fEnable: 1,         // TRUE
                hRgnBlur: h_rgn,
                fTransitionOnMaximized: 0,
            };
            let hr = DwmEnableBlurBehindWindow(hwnd as HWND, &bb);
            println!("[DesktopEmbedder] DwmEnableBlurBehindWindow: hr=0x{:X}", hr);
        }

        // SetLayeredWindowAttributes - 完全不透明
        SetLayeredWindowAttributes(hwnd as HWND, 0, 0xFF, LWA_ALPHA);
        println!("[DesktopEmbedder] SetLayeredWindowAttributes: alpha=0xFF");

        // ===== 8. SetParent 到目标窗口 =====
        let prev_parent = SetParent(hwnd as HWND, embed_target);
        println!(
            "[DesktopEmbedder] SetParent: HWND 0x{:X} -> target 0x{:X} (prev_parent=0x{:X})",
            hwnd, embed_target as isize, prev_parent as isize
        );

        // ===== 9. Z-order 管理（参照博主代码） =====
        // 壁纸 → HWND_TOP
        SetWindowPos(
            hwnd as HWND, HWND_TOP, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_DRAWFRAME,
        );
        // DefView → HWND_TOP（覆盖在壁纸之上，确保图标可见）
        SetWindowPos(
            defview, HWND_TOP, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        // WorkerW → HWND_BOTTOM（系统壁纸沉底）
        SetWindowPos(
            effective_worker, HWND_BOTTOM, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_DRAWFRAME,
        );
        println!("[DesktopEmbedder] Z-order set: DefView > EmbedWnd > WorkerW");

        // ===== 10. 定位窗口到目标显示器（含 NC offset 补偿） =====
        //
        // 即使移除了 WS_CAPTION/WS_THICKFRAME 并禁用了 DWM NC 渲染，
        // Windows 仍可能保留残余的非客户区边框。
        // 策略：先 MoveWindow 到目标尺寸，然后用 GetWindowRect + ClientToScreen
        // 检测实际 NC 边距，如果存在偏移则扩大窗口并用负偏移补偿。
        {
            use windows_sys::Win32::Foundation::{POINT, RECT};
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                ClientToScreen, GetClientRect, GetWindowRect,
            };

            // 先按目标尺寸定位
            MoveWindow(
                hwnd as HWND,
                monitor_x, monitor_y,
                monitor_width, monitor_height,
                1,
            );

            // 检测 NC 偏移：比较窗口矩形和客户区矩形
            let mut window_rect: RECT = std::mem::zeroed();
            let mut client_rect: RECT = std::mem::zeroed();
            GetWindowRect(hwnd as HWND, &mut window_rect);
            GetClientRect(hwnd as HWND, &mut client_rect);

            let window_w = window_rect.right - window_rect.left;
            let window_h = window_rect.bottom - window_rect.top;
            let client_w = client_rect.right - client_rect.left;
            let client_h = client_rect.bottom - client_rect.top;

            if client_w != monitor_width || client_h != monitor_height {
                // 存在 NC 偏移，需要补偿
                // 用 ClientToScreen 将客户区原点 (0,0) 转换为屏幕坐标，
                // 与窗口矩形对比即可得到各边的 NC 边距
                let mut client_origin = POINT { x: 0, y: 0 };
                ClientToScreen(hwnd as HWND, &mut client_origin);

                let nc_left = client_origin.x - window_rect.left;
                let nc_top = client_origin.y - window_rect.top;
                let nc_right = window_w - client_w - nc_left;
                let nc_bottom = window_h - client_h - nc_top;

                println!(
                    "[DesktopEmbedder] NC offset detected: L={} T={} R={} B={}, compensating...",
                    nc_left, nc_top, nc_right, nc_bottom
                );

                // 扩大窗口以补偿 NC 边距，使客户区精确覆盖显示器
                let comp_x = monitor_x - nc_left;
                let comp_y = monitor_y - nc_top;
                let comp_w = monitor_width + nc_left + nc_right;
                let comp_h = monitor_height + nc_top + nc_bottom;

                MoveWindow(hwnd as HWND, comp_x, comp_y, comp_w, comp_h, 1);

                println!(
                    "[DesktopEmbedder] Compensated → pos=({}, {}), size={}x{}",
                    comp_x, comp_y, comp_w, comp_h
                );
            } else {
                println!(
                    "[DesktopEmbedder] No NC offset, MoveWindow: pos=({},{}), size={}x{}",
                    monitor_x, monitor_y, monitor_width, monitor_height
                );
            }
        }

        // ===== 11. 显示窗口 =====
        ShowWindow(progman, SW_SHOW);
        ShowWindow(hwnd as HWND, SW_SHOW);
        ShowWindow(effective_worker, SW_SHOW);

        // ===== 12. 注册到全局列表并启动 Z-order 监控 =====
        {
            let mut guard = EMBEDDED_WINDOWS.lock().unwrap();
            let list = guard.get_or_insert_with(Vec::new);
            list.retain(|w| w.hwnd != hwnd);
            list.push(EmbeddedWindow {
                hwnd,
                monitor_x,
                monitor_y,
                monitor_w: monitor_width,
                monitor_h: monitor_height,
            });
            println!(
                "[DesktopEmbedder] Registered embedded window: HWND 0x{:X}, total={}",
                hwnd, list.len()
            );
        }

        // 启动 Z-order 监控定时器（所有模式都需要）
        start_zorder_timer();

        println!(
            "[DesktopEmbedder] Embedded HWND 0x{:X} into 0x{:X} (pos=({},{}), size={}x{})",
            hwnd, embed_target as isize, monitor_x, monitor_y, monitor_width, monitor_height
        );

        Ok(())
    }
}

/// 启动 Z-order 监控定时器
///
/// 每 500ms 检查壁纸窗口是否仍在 DefView 正下方。
/// 如果被其他窗口插入，自动修正。
/// 连续 5 次检测到同一冲突窗口则停止（可能存在竞争）。
#[cfg(target_os = "windows")]
fn start_zorder_timer() {
    if ZORDER_TIMER_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    std::thread::spawn(|| {
        println!("[DesktopEmbedder] Z-order monitor timer started (interval=500ms)");

        const MAX_CONSECUTIVE_FIXES: i32 = 5;
        let mut last_conflict: HWND = std::ptr::null_mut();
        let mut consecutive_count: i32 = 0;

        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));

            let windows = {
                let guard = EMBEDDED_WINDOWS.lock().unwrap();
                match guard.as_ref() {
                    Some(list) if !list.is_empty() => list.clone(),
                    _ => {
                        ZORDER_TIMER_RUNNING.store(false, Ordering::SeqCst);
                        println!("[DesktopEmbedder] Z-order timer stopped (no embedded windows)");
                        return;
                    }
                }
            };

            let defview = CACHED_DEFVIEW.load(Ordering::SeqCst) as HWND;
            if defview == std::ptr::null_mut() {
                continue;
            }

            unsafe {
                for ew in &windows {
                    let (ok, conflict) = ensure_embed_below_defview(defview, ew.hwnd as HWND);

                    if !ok {
                        if conflict == std::ptr::null_mut() {
                            consecutive_count = 0;
                        } else if conflict == last_conflict {
                            consecutive_count += 1;
                        } else {
                            last_conflict = conflict;
                            consecutive_count = 1;
                        }

                        if conflict != std::ptr::null_mut() {
                            println!(
                                "[DesktopEmbedder] Z-order conflict: HWND 0x{:X}, count={}",
                                conflict as isize, consecutive_count
                            );
                        }

                        if consecutive_count >= MAX_CONSECUTIVE_FIXES {
                            println!(
                                "[DesktopEmbedder] ERROR: Repeated Z-order conflict! Stopping monitor."
                            );
                            ZORDER_TIMER_RUNNING.store(false, Ordering::SeqCst);
                            return;
                        }
                    } else {
                        consecutive_count = 0;
                        last_conflict = std::ptr::null_mut();
                    }
                }
            }
        }
    });
}

/// 从桌面层级中移除嵌入的窗口
#[cfg(target_os = "windows")]
pub fn unembed_from_desktop(hwnd: isize) {
    {
        let mut guard = EMBEDDED_WINDOWS.lock().unwrap();
        if let Some(list) = guard.as_mut() {
            list.retain(|w| w.hwnd != hwnd);
            println!(
                "[DesktopEmbedder] Unregistered HWND 0x{:X}, remaining={}",
                hwnd, list.len()
            );
        }
    }
    unsafe {
        SetParent(hwnd as HWND, std::ptr::null_mut());
        println!("[DesktopEmbedder] Unembedded HWND 0x{:X}", hwnd);
    }
}

/// 停止 Z-order 监控定时器并清理所有嵌入窗口记录
#[cfg(target_os = "windows")]
pub fn cleanup_all() {
    {
        let mut guard = EMBEDDED_WINDOWS.lock().unwrap();
        if let Some(list) = guard.as_mut() {
            list.clear();
        }
    }
    CACHED_DEFVIEW.store(0, Ordering::SeqCst);
    CACHED_EMBED_TARGET.store(0, Ordering::SeqCst);
    CACHED_WORKERW.store(0, Ordering::SeqCst);
    IS_LEGACY_MODE.store(false, Ordering::SeqCst);
    println!("[DesktopEmbedder] Cleanup: all embedded windows cleared");
}

// ===== 非 Windows 平台的空实现 =====

#[cfg(not(target_os = "windows"))]
pub fn embed_in_desktop(
    _hwnd: isize,
    _monitor_x: i32,
    _monitor_y: i32,
    _monitor_width: i32,
    _monitor_height: i32,
) -> Result<(), String> {
    println!("[DesktopEmbedder] embed_in_desktop is a no-op on this platform");
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn unembed_from_desktop(_hwnd: isize) {
    println!("[DesktopEmbedder] unembed_from_desktop is a no-op on this platform");
}

#[cfg(not(target_os = "windows"))]
pub fn cleanup_all() {
    println!("[DesktopEmbedder] cleanup_all is a no-op on this platform");
}