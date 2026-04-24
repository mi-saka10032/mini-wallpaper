//! Win32 桌面壁纸嵌入模块 — Direct Child 方案
//!
//! 核心原理（Direct Child）：
//! 1. FindWindow("Progman") 找到桌面窗口
//! 2. SendMessageTimeout(Progman, 0x052C) 触发 explorer 创建 WorkerW
//! 3. SetParent(tauri_hwnd, progman) 将壁纸窗口直接作为 Progman 的子窗口
//! 4. 通过 SetWindowPos 将壁纸窗口的 Z-order 调整到 SHELLDLL_DefView 之下
//! 5. 启动定时器持续监控 Z-order，确保壁纸始终在图标层之下
//!
//! 为什么选择 Direct Child 方案：
//! Windows 11 24H2 在 SetParent 到 WorkerW 后会通过 WM_NCCALCSIZE 注入隐藏的 NC 边框（~8px），
//! 导致壁纸窗口出现偏移和黏着。经过多轮验证（WndProc 子类化、NC 补偿、DWM 属性等），
//! 均无法完美解决。Direct Child 方案将窗口直接挂到 Progman 下，绕过 WorkerW 的 NC 注入问题，
//! 并通过定时器维护 Z-order 稳定性。

#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GetWindow, MoveWindow, SendMessageTimeoutW,
    SetParent, SetWindowPos, SMTO_NORMAL,
};

#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
#[cfg(target_os = "windows")]
use std::sync::Mutex;

// ===== Z-order 监控相关全局状态 =====

/// 已嵌入的壁纸窗口列表（HWND, monitor_x, monitor_y, monitor_w, monitor_h）
#[cfg(target_os = "windows")]
static EMBEDDED_WINDOWS: Mutex<Option<Vec<EmbeddedWindow>>> = Mutex::new(None);

/// Z-order 监控定时器是否已启动
#[cfg(target_os = "windows")]
static ZORDER_TIMER_RUNNING: AtomicBool = AtomicBool::new(false);

/// Progman 窗口句柄缓存
#[cfg(target_os = "windows")]
static CACHED_PROGMAN: AtomicIsize = AtomicIsize::new(0);

#[cfg(target_os = "windows")]
#[derive(Clone, Debug)]
struct EmbeddedWindow {
    hwnd: isize,
    monitor_x: i32,
    monitor_y: i32,
    monitor_w: i32,
    monitor_h: i32,
}

/// 在 Windows 上将壁纸窗口作为 Progman 的直接子窗口嵌入桌面
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
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW, GWL_EXSTYLE, GWL_STYLE,
        LWA_ALPHA, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
        WS_BORDER, WS_CAPTION, WS_CHILD, WS_DLGFRAME, WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME,
        WS_EX_LAYERED, WS_EX_STATICEDGE, WS_EX_TRANSPARENT, WS_EX_WINDOWEDGE, WS_THICKFRAME,
        WS_VISIBLE,
    };

    unsafe {
        println!(
            "[DesktopEmbedder] Monitor rect: ({}, {}) {}x{}",
            monitor_x, monitor_y, monitor_width, monitor_height
        );

        // ===== 1. 找到 Progman 窗口 =====
        let progman = FindWindowW(encode_wide("Progman\0").as_ptr(), std::ptr::null());
        if progman.is_null() {
            return Err("Failed to find Progman window".into());
        }
        CACHED_PROGMAN.store(progman as isize, Ordering::SeqCst);

        // ===== 2. 发送 0x052C 消息触发 WorkerW 创建 =====
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
        println!("[DesktopEmbedder] Sent 0x052C to Progman, WorkerW should be created");

        // ===== 3. 准备窗口样式（SetParent 之前） =====

        // 3a. 禁用 DWM 圆角（顶层窗口时设置有效）
        {
            const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
            const DWMWCP_DONOTROUND: u32 = 1;
            let corner_pref: u32 = DWMWCP_DONOTROUND;
            let hr = DwmSetWindowAttribute(
                hwnd as HWND,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner_pref as *const u32 as *const _,
                std::mem::size_of::<u32>() as u32,
            );
            if hr == 0 {
                println!("[DesktopEmbedder] DWM: Corner preference set to DONOTROUND");
            } else {
                println!(
                    "[DesktopEmbedder] DWM: DWMWA_WINDOW_CORNER_PREFERENCE not supported (hr=0x{:X})",
                    hr
                );
            }
        }

        // 3b. 禁用 DWM NC 渲染
        {
            const DWMWA_NCRENDERING_POLICY: u32 = 2;
            const DWMNCRP_DISABLED: u32 = 1;
            let ncrp: u32 = DWMNCRP_DISABLED;
            DwmSetWindowAttribute(
                hwnd as HWND,
                DWMWA_NCRENDERING_POLICY,
                &ncrp as *const u32 as *const _,
                std::mem::size_of::<u32>() as u32,
            );
            println!("[DesktopEmbedder] DWM: NC rendering policy set to DISABLED");
        }

        // 3c. 设置 DWM 透明化（Cloak）— 让 DWM 将窗口视为透明合成层
        //     DWMWA_CLOAK = 13, 值 = 2 (DWM_CLOAKED_APP)
        //     这可以帮助消除 DWM 在子窗口边缘绘制的任何装饰
        {
            // 先 cloak，SetParent 后再 uncloak
            const DWMWA_CLOAK: u32 = 13;
            let cloak_val: u32 = 2;
            let _ = DwmSetWindowAttribute(
                hwnd as HWND,
                DWMWA_CLOAK,
                &cloak_val as *const u32 as *const _,
                std::mem::size_of::<u32>() as u32,
            );
        }

        // 3d. 清除窗口样式中的所有边框位，设置为子窗口
        let style = GetWindowLongPtrW(hwnd as HWND, GWL_STYLE);
        let clean_style = (style
            & !(WS_CAPTION as isize)
            & !(WS_THICKFRAME as isize)
            & !(WS_BORDER as isize)
            & !(WS_DLGFRAME as isize))
            | WS_CHILD as isize
            | WS_VISIBLE as isize;
        SetWindowLongPtrW(hwnd as HWND, GWL_STYLE, clean_style);

        // 3e. 设置扩展样式：WS_EX_LAYERED + WS_EX_TRANSPARENT（鼠标穿透）
        let ex_style = GetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE);
        let clean_ex = (ex_style
            & !(WS_EX_CLIENTEDGE as isize)
            & !(WS_EX_STATICEDGE as isize)
            & !(WS_EX_WINDOWEDGE as isize)
            & !(WS_EX_DLGMODALFRAME as isize))
            | WS_EX_LAYERED as isize
            | WS_EX_TRANSPARENT as isize;
        SetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE, clean_ex);

        // 3f. 设置 Layered 窗口属性：完全不透明
        SetLayeredWindowAttributes(hwnd as HWND, 0, 0xFF, LWA_ALPHA);

        // 3g. SWP_FRAMECHANGED 强制系统重新计算窗口框架
        SetWindowPos(
            hwnd as HWND,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE,
        );

        println!("[DesktopEmbedder] Pre-SetParent: styles cleaned, frame changed");

        // ===== 4. SetParent 到 Progman（核心操作） =====
        //
        // 与经典方案不同，我们不是嵌入到 WorkerW，而是直接嵌入到 Progman。
        // 这样壁纸窗口和 SHELLDLL_DefView 成为兄弟窗口，
        // 通过 Z-order 控制壁纸在图标层之下。
        let prev_parent = SetParent(hwnd as HWND, progman);
        if prev_parent.is_null() {
            return Err("SetParent to Progman failed".into());
        }
        println!(
            "[DesktopEmbedder] SetParent success: HWND 0x{:X} -> Progman 0x{:X}",
            hwnd, progman as isize
        );

        // 4a. Uncloak 窗口
        {
            const DWMWA_CLOAK: u32 = 13;
            let uncloak_val: u32 = 0;
            let _ = DwmSetWindowAttribute(
                hwnd as HWND,
                DWMWA_CLOAK,
                &uncloak_val as *const u32 as *const _,
                std::mem::size_of::<u32>() as u32,
            );
        }

        // 4b. 再次设置 DWM NC 渲染策略（SetParent 可能重置）
        {
            const DWMWA_NCRENDERING_POLICY: u32 = 2;
            const DWMNCRP_DISABLED: u32 = 1;
            let ncrp: u32 = DWMNCRP_DISABLED;
            DwmSetWindowAttribute(
                hwnd as HWND,
                DWMWA_NCRENDERING_POLICY,
                &ncrp as *const u32 as *const _,
                std::mem::size_of::<u32>() as u32,
            );
        }

        // 4c. 再次 SWP_FRAMECHANGED
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
        MoveWindow(
            hwnd as HWND,
            monitor_x,
            monitor_y,
            monitor_width,
            monitor_height,
            1,
        );

        // ===== 6. 调整 Z-order：壁纸窗口放到 SHELLDLL_DefView 之下 =====
        fix_zorder_for_hwnd(progman, hwnd as HWND);

        // ===== 7. 注册到全局列表并启动 Z-order 监控定时器 =====
        {
            let mut guard = EMBEDDED_WINDOWS.lock().unwrap();
            let list = guard.get_or_insert_with(Vec::new);
            // 移除已存在的同一 HWND（防止重复注册）
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
                hwnd,
                list.len()
            );
        }

        // 启动 Z-order 监控定时器（如果尚未启动）
        start_zorder_timer();

        println!(
            "[DesktopEmbedder] Embedded HWND 0x{:X} into Progman 0x{:X} (pos=({},{}), size={}x{})",
            hwnd, progman as isize, monitor_x, monitor_y, monitor_width, monitor_height
        );

        Ok(())
    }
}

/// 修正单个壁纸窗口的 Z-order，使其位于 SHELLDLL_DefView 之下
#[cfg(target_os = "windows")]
unsafe fn fix_zorder_for_hwnd(progman: HWND, wallpaper_hwnd: HWND) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GW_HWNDNEXT, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };

    // 在 Progman 的子窗口中找到 SHELLDLL_DefView
    let defview = FindWindowExW(
        progman,
        std::ptr::null_mut(),
        encode_wide("SHELLDLL_DefView\0").as_ptr(),
        std::ptr::null(),
    );

    if defview.is_null() {
        // SHELLDLL_DefView 可能在 WorkerW 中（经典布局），尝试在 WorkerW 中查找
        let workerw = find_workerw_with_defview();
        if !workerw.is_null() {
            // 如果 DefView 在 WorkerW 中，我们需要把壁纸窗口放到该 WorkerW 之下
            SetWindowPos(
                wallpaper_hwnd,
                workerw,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            println!(
                "[DesktopEmbedder] Z-order: HWND 0x{:X} placed after WorkerW(DefView) 0x{:X}",
                wallpaper_hwnd as isize, workerw as isize
            );
        } else {
            println!("[DesktopEmbedder] WARNING: Cannot find SHELLDLL_DefView anywhere!");
        }
        return;
    }

    // SHELLDLL_DefView 在 Progman 中（Direct Child 布局）
    // 将壁纸窗口放到 DefView 之后（Z-order 更低 = 视觉上在下面）
    SetWindowPos(
        wallpaper_hwnd,
        defview,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
    );
    println!(
        "[DesktopEmbedder] Z-order: HWND 0x{:X} placed after DefView 0x{:X}",
        wallpaper_hwnd as isize, defview as isize
    );
}

/// 查找包含 SHELLDLL_DefView 的 WorkerW 窗口
#[cfg(target_os = "windows")]
unsafe fn find_workerw_with_defview() -> HWND {
    static FOUND: AtomicIsize = AtomicIsize::new(0);
    FOUND.store(0, Ordering::SeqCst);

    unsafe extern "system" fn enum_cb(hwnd: HWND, _: LPARAM) -> BOOL {
        let defview = FindWindowExW(
            hwnd,
            std::ptr::null_mut(),
            encode_wide("SHELLDLL_DefView\0").as_ptr(),
            std::ptr::null(),
        );
        if !defview.is_null() {
            FOUND.store(hwnd as isize, Ordering::SeqCst);
            return 0; // 停止枚举
        }
        1
    }

    EnumWindows(Some(enum_cb), 0);
    FOUND.load(Ordering::SeqCst) as HWND
}

/// 启动 Z-order 监控定时器
///
/// 每 500ms 检查一次所有嵌入窗口的 Z-order 是否正确，
/// 如果被其他操作打乱则自动修正。
#[cfg(target_os = "windows")]
fn start_zorder_timer() {
    if ZORDER_TIMER_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        // 定时器已在运行
        return;
    }

    std::thread::spawn(|| {
        println!("[DesktopEmbedder] Z-order monitor timer started (interval=500ms)");

        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));

            // 检查是否还有嵌入窗口
            let windows = {
                let guard = EMBEDDED_WINDOWS.lock().unwrap();
                match guard.as_ref() {
                    Some(list) if !list.is_empty() => list.clone(),
                    _ => {
                        // 没有嵌入窗口了，停止定时器
                        ZORDER_TIMER_RUNNING.store(false, Ordering::SeqCst);
                        println!("[DesktopEmbedder] Z-order monitor timer stopped (no embedded windows)");
                        return;
                    }
                }
            };

            let progman_val = CACHED_PROGMAN.load(Ordering::SeqCst);
            if progman_val == 0 {
                continue;
            }
            let progman = progman_val as HWND;

            unsafe {
                // 找到 SHELLDLL_DefView 的位置（可能在 Progman 或 WorkerW 中）
                let defview_in_progman = FindWindowExW(
                    progman,
                    std::ptr::null_mut(),
                    encode_wide("SHELLDLL_DefView\0").as_ptr(),
                    std::ptr::null(),
                );

                for ew in &windows {
                    if defview_in_progman.is_null() {
                        // DefView 在 WorkerW 中，找到那个 WorkerW
                        let workerw = find_workerw_with_defview();
                        if !workerw.is_null() {
                            // 检查壁纸窗口是否在 WorkerW 之后
                            if !is_hwnd_after(ew.hwnd as HWND, workerw, progman) {
                                use windows_sys::Win32::UI::WindowsAndMessaging::{
                                    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
                                };
                                SetWindowPos(
                                    ew.hwnd as HWND,
                                    workerw,
                                    0,
                                    0,
                                    0,
                                    0,
                                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                                );
                                println!(
                                    "[DesktopEmbedder] Z-order fix: HWND 0x{:X} repositioned after WorkerW 0x{:X}",
                                    ew.hwnd, workerw as isize
                                );
                            }
                        }
                    } else {
                        // DefView 在 Progman 中，壁纸应在 DefView 之后
                        if !is_hwnd_after(ew.hwnd as HWND, defview_in_progman, progman) {
                            use windows_sys::Win32::UI::WindowsAndMessaging::{
                                SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
                            };
                            SetWindowPos(
                                ew.hwnd as HWND,
                                defview_in_progman,
                                0,
                                0,
                                0,
                                0,
                                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                            );
                            println!(
                                "[DesktopEmbedder] Z-order fix: HWND 0x{:X} repositioned after DefView 0x{:X}",
                                ew.hwnd, defview_in_progman as isize
                            );
                        }
                    }
                }
            }
        }
    });
}

/// 检查 target_hwnd 是否在 after_hwnd 的紧后面（Z-order 上更低）
///
/// 在父窗口的子窗口列表中，Z-order 从高到低排列。
/// 我们需要确保 target_hwnd 在 after_hwnd 之后（即 Z-order 更低）。
#[cfg(target_os = "windows")]
unsafe fn is_hwnd_after(target_hwnd: HWND, after_hwnd: HWND, _parent: HWND) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GW_HWNDNEXT;

    // 从 after_hwnd 开始往下遍历，看 target_hwnd 是否紧跟其后
    let next = GetWindow(after_hwnd, GW_HWNDNEXT);
    if next == target_hwnd {
        return true;
    }

    // 如果不是紧跟其后，检查是否在 after_hwnd 之后的任意位置
    // （允许中间有其他壁纸窗口）
    let mut current = next;
    while !current.is_null() {
        if current == target_hwnd {
            return true;
        }
        current = GetWindow(current, GW_HWNDNEXT);
    }

    false
}

/// 从桌面层级中移除嵌入的窗口
#[cfg(target_os = "windows")]
pub fn unembed_from_desktop(hwnd: isize) {
    // 从全局列表中移除
    {
        let mut guard = EMBEDDED_WINDOWS.lock().unwrap();
        if let Some(list) = guard.as_mut() {
            list.retain(|w| w.hwnd != hwnd);
            println!(
                "[DesktopEmbedder] Unregistered HWND 0x{:X}, remaining={}",
                hwnd,
                list.len()
            );
        }
    }

    unsafe {
        // SetParent(hwnd, NULL) 将窗口还原为顶层窗口
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
    // 定时器会在下次循环时检测到空列表并自动退出
    println!("[DesktopEmbedder] Cleanup: all embedded windows cleared");
}

/// 辅助函数：将 &str 编码为以 null 结尾的 UTF-16 Vec
#[cfg(target_os = "windows")]
fn encode_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
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
