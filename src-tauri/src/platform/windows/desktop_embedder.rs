//! Win32 桌面壁纸嵌入模块 — 混合方案
//!
//! 核心原理：
//! 1. FindWindow("Progman") 找到桌面窗口
//! 2. SendMessageTimeout(Progman, 0x052C) 触发 explorer 创建 WorkerW
//! 3. 检测 SHELLDLL_DefView 的位置：
//!    - 情况 A（DefView 在 WorkerW 中）：壁纸嵌入 Progman，无需额外 Z-order 处理
//!    - 情况 B（DefView 仍在 Progman 中）：找到空 WorkerW 并嵌入，壁纸自然在图标之下
//! 4. 启动定时器持续监控 Z-order 稳定性
//!
//! Windows 11 24H2 的 NC 边框问题：
//! 24H2 在 SetParent 后会通过 WM_NCCALCSIZE 注入隐藏的 NC 边框（~8px）。
//! 解决方案：在 SetParent 前后彻底清除所有边框样式，禁用 DWM NC 渲染，
//! 并通过 WS_POPUP 替代 WS_CHILD 来避免系统注入 NC 区域。

#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, MoveWindow, SendMessageTimeoutW, SetParent,
    SetWindowPos, SMTO_NORMAL,
};

#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
#[cfg(target_os = "windows")]
use std::sync::Mutex;

// ===== Z-order 监控相关全局状态 =====

/// 已嵌入的壁纸窗口列表
#[cfg(target_os = "windows")]
static EMBEDDED_WINDOWS: Mutex<Option<Vec<EmbeddedWindow>>> = Mutex::new(None);

/// Z-order 监控定时器是否已启动
#[cfg(target_os = "windows")]
static ZORDER_TIMER_RUNNING: AtomicBool = AtomicBool::new(false);

/// 嵌入目标窗口句柄缓存（可能是 Progman 或 WorkerW）
#[cfg(target_os = "windows")]
static CACHED_PARENT: AtomicIsize = AtomicIsize::new(0);

/// 嵌入模式：0=未确定, 1=Progman(DefView在WorkerW中), 2=WorkerW(DefView在Progman中)
#[cfg(target_os = "windows")]
static EMBED_MODE: AtomicIsize = AtomicIsize::new(0);

#[cfg(target_os = "windows")]
#[derive(Clone, Debug)]
struct EmbeddedWindow {
    hwnd: isize,
    monitor_x: i32,
    monitor_y: i32,
    monitor_w: i32,
    monitor_h: i32,
}

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
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW, GWL_EXSTYLE, GWL_STYLE,
        LWA_ALPHA, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
        SW_SHOW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_DLGFRAME, WS_EX_CLIENTEDGE,
        WS_EX_DLGMODALFRAME, WS_EX_LAYERED, WS_EX_STATICEDGE, WS_EX_TRANSPARENT,
        WS_EX_WINDOWEDGE, WS_THICKFRAME, WS_VISIBLE,
    };

    unsafe {
        println!(
            "[DesktopEmbedder] Monitor rect: ({}, {}) {}x{}",
            monitor_x, monitor_y, monitor_width, monitor_height
        );

        // ===== 1. 找到 Progman 窗口 =====
        let progman = FindWindowW(encode_wide("Progman\0").as_ptr(), std::ptr::null());
        if progman == std::ptr::null_mut() {
            return Err("Failed to find Progman window".into());
        }
        println!("[DesktopEmbedder] Found Progman: 0x{:X}", progman as isize);

        // ===== 2. 发送 0x052C 消息触发 WorkerW 创建 =====
        let mut _result: usize = 0;
        SendMessageTimeoutW(
            progman,
            0x052C,
            0xD,
            0x1,
            SMTO_NORMAL,
            1000,
            &mut _result as *mut usize,
        );
        println!("[DesktopEmbedder] Sent 0x052C to Progman");

        // 短暂等待 explorer 处理消息
        std::thread::sleep(std::time::Duration::from_millis(100));

        // ===== 3. 确定嵌入目标 =====
        //
        // 检测 SHELLDLL_DefView 的位置来决定嵌入策略：
        //
        // 情况 A：DefView 在某个 WorkerW 中（0x052C 生效）
        //   层级：WorkerW(DefView/图标) > Progman(壁纸窗口)
        //   → 壁纸嵌入 Progman，自然在图标之下
        //
        // 情况 B：DefView 仍在 Progman 中（0x052C 未迁移 DefView）
        //   层级：WorkerW(空) 在 Progman 之后
        //   → 壁纸嵌入空 WorkerW，但需要处理 Z-order
        //   → 或者直接嵌入 Progman，壁纸放在 DefView 之上，用 WS_EX_TRANSPARENT 穿透鼠标

        let defview_in_progman = FindWindowExW(
            progman,
            std::ptr::null_mut(),
            encode_wide("SHELLDLL_DefView\0").as_ptr(),
            std::ptr::null(),
        );

        let workerw_with_defview = find_workerw_with_defview();

        let (embed_target, mode_desc) = if workerw_with_defview != std::ptr::null_mut()
            && defview_in_progman == std::ptr::null_mut()
        {
            // 情况 A：DefView 已迁移到 WorkerW，嵌入 Progman
            println!(
                "[DesktopEmbedder] Mode A: DefView in WorkerW 0x{:X}, embedding into Progman",
                workerw_with_defview as isize
            );
            EMBED_MODE.store(1, Ordering::SeqCst);
            (progman, "Progman")
        } else if defview_in_progman != std::ptr::null_mut() {
            // 情况 B：DefView 仍在 Progman 中
            // 找到 0x052C 创建的空 WorkerW
            let empty_workerw = find_empty_workerw(progman);
            if empty_workerw != std::ptr::null_mut() {
                println!(
                    "[DesktopEmbedder] Mode B: DefView in Progman, embedding into empty WorkerW 0x{:X}",
                    empty_workerw as isize
                );
                EMBED_MODE.store(2, Ordering::SeqCst);
                (empty_workerw, "WorkerW")
            } else {
                // 没有空 WorkerW，回退到 Progman 并将壁纸放在 DefView 之上
                println!(
                    "[DesktopEmbedder] Mode C: DefView in Progman, no empty WorkerW, embedding into Progman above DefView"
                );
                EMBED_MODE.store(3, Ordering::SeqCst);
                (progman, "Progman(above-defview)")
            }
        } else {
            // 兜底：嵌入 Progman
            println!("[DesktopEmbedder] Mode D: Fallback, embedding into Progman");
            EMBED_MODE.store(1, Ordering::SeqCst);
            (progman, "Progman")
        };

        CACHED_PARENT.store(embed_target as isize, Ordering::SeqCst);
        println!(
            "[DesktopEmbedder] Embed target: {} (0x{:X})",
            mode_desc, embed_target as isize
        );

        // ===== 4. 准备窗口样式（SetParent 之前） =====

        // 4a. 禁用 DWM NC 渲染
        {
            const DWMWA_NCRENDERING_POLICY: u32 = 2;
            const DWMNCRP_DISABLED: u32 = 1;
            let ncrp: u32 = DWMNCRP_DISABLED;
            let hr = DwmSetWindowAttribute(
                hwnd as HWND,
                DWMWA_NCRENDERING_POLICY,
                &ncrp as *const u32 as *const _,
                std::mem::size_of::<u32>() as u32,
            );
            println!(
                "[DesktopEmbedder] DWM: NC rendering policy set to DISABLED (hr=0x{:X})",
                hr
            );
        }

        // 4b. 清除窗口样式中的所有边框位，设置为子窗口
        let style = GetWindowLongPtrW(hwnd as HWND, GWL_STYLE);
        let clean_style = (style
            & !(WS_CAPTION as isize)
            & !(WS_THICKFRAME as isize)
            & !(WS_BORDER as isize)
            & !(WS_DLGFRAME as isize))
            | WS_CHILD as isize
            | WS_VISIBLE as isize;
        SetWindowLongPtrW(hwnd as HWND, GWL_STYLE, clean_style);

        // 4c. 设置扩展样式
        let ex_style = GetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE);
        let clean_ex = ex_style
            & !(WS_EX_CLIENTEDGE as isize)
            & !(WS_EX_STATICEDGE as isize)
            & !(WS_EX_WINDOWEDGE as isize)
            & !(WS_EX_DLGMODALFRAME as isize)
            & !(WS_EX_LAYERED as isize)
            & !(WS_EX_TRANSPARENT as isize);
        SetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE, clean_ex);

        // 4d. SWP_FRAMECHANGED 强制系统重新计算窗口框架
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

        // ===== 5. SetParent 到目标窗口 =====
        let prev_parent = SetParent(hwnd as HWND, embed_target);
        if prev_parent == std::ptr::null_mut() {
            return Err(format!("SetParent to {} failed", mode_desc));
        }
        println!(
            "[DesktopEmbedder] SetParent success: HWND 0x{:X} -> {} 0x{:X}",
            hwnd, mode_desc, embed_target as isize
        );

        // 5a. SetParent 后再次清除样式（SetParent 可能重置部分样式）
        let style_after = GetWindowLongPtrW(hwnd as HWND, GWL_STYLE);
        let clean_style_after = (style_after
            & !(WS_CAPTION as isize)
            & !(WS_THICKFRAME as isize)
            & !(WS_BORDER as isize)
            & !(WS_DLGFRAME as isize))
            | WS_CHILD as isize
            | WS_VISIBLE as isize;
        SetWindowLongPtrW(hwnd as HWND, GWL_STYLE, clean_style_after);

        // 5b. 再次禁用 DWM NC 渲染
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

        // 5c. 再次 SWP_FRAMECHANGED
        SetWindowPos(
            hwnd as HWND,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE,
        );

        // ===== 6. 检查 NC 偏移并补偿 =====
        //
        // 在 24H2 上，即使清除了所有样式，SetParent 后仍可能有 NC 偏移。
        // 通过比较 window rect 和 client rect 来检测并补偿。
        let (final_x, final_y, final_w, final_h) = {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                GetClientRect, GetWindowRect,
            };
            use windows_sys::Win32::Foundation::RECT;

            let mut window_rect: RECT = std::mem::zeroed();
            let mut client_rect: RECT = std::mem::zeroed();
            GetWindowRect(hwnd as HWND, &mut window_rect);
            GetClientRect(hwnd as HWND, &mut client_rect);

            let window_w = window_rect.right - window_rect.left;
            let window_h = window_rect.bottom - window_rect.top;
            let client_w = client_rect.right - client_rect.left;
            let client_h = client_rect.bottom - client_rect.top;

            if client_w < window_w || client_h < window_h {
                // 存在 NC 偏移，需要补偿
                let nc_left = (window_w - client_w) / 2;
                let nc_top = window_h - client_h - nc_left; // 顶部通常比侧边小
                let nc_right = nc_left;
                let nc_bottom = window_h - client_h - nc_top;

                println!(
                    "[DesktopEmbedder] NC offset detected: L={} T={} R={} B={}, window={}x{}, client={}x{}",
                    nc_left, nc_top, nc_right, nc_bottom, window_w, window_h, client_w, client_h
                );

                // 补偿：扩大窗口并偏移位置
                let comp_x = monitor_x - nc_left;
                let comp_y = monitor_y - nc_top;
                let comp_w = monitor_width + nc_left + nc_right;
                let comp_h = monitor_height + nc_top + nc_bottom;

                println!(
                    "[DesktopEmbedder] Compensated: pos=({},{}), size={}x{}",
                    comp_x, comp_y, comp_w, comp_h
                );
                (comp_x, comp_y, comp_w, comp_h)
            } else {
                println!("[DesktopEmbedder] No NC offset detected, using exact dimensions");
                (monitor_x, monitor_y, monitor_width, monitor_height)
            }
        };

        // ===== 7. 定位窗口到目标显示器区域 =====
        MoveWindow(hwnd as HWND, final_x, final_y, final_w, final_h, 1);
        println!(
            "[DesktopEmbedder] MoveWindow: pos=({},{}), size={}x{}",
            final_x, final_y, final_w, final_h
        );

        // ===== 8. 确保窗口可见 =====
        ShowWindow(hwnd as HWND, SW_SHOW);

        // ===== 9. 处理 Z-order =====
        let mode = EMBED_MODE.load(Ordering::SeqCst);
        if mode == 3 {
            // Mode C：壁纸在 Progman 中，DefView 也在 Progman 中
            // 需要将壁纸放到 DefView 之上，并设置鼠标穿透
            let ex = GetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE);
            SetWindowLongPtrW(
                hwnd as HWND,
                GWL_EXSTYLE,
                ex | WS_EX_TRANSPARENT as isize,
            );

            // 将壁纸放到 Progman 子窗口列表的最顶部
            use windows_sys::Win32::UI::WindowsAndMessaging::HWND_TOP;
            SetWindowPos(
                hwnd as HWND,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            println!(
                "[DesktopEmbedder] Z-order: Mode C - placed above DefView with WS_EX_TRANSPARENT"
            );
        }
        // Mode A 和 Mode B 不需要额外的 Z-order 处理

        // ===== 10. 注册到全局列表并启动 Z-order 监控定时器 =====
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
                hwnd,
                list.len()
            );
        }

        start_zorder_timer();

        println!(
            "[DesktopEmbedder] Embedded HWND 0x{:X} into {} 0x{:X} (pos=({},{}), size={}x{})",
            hwnd, mode_desc, embed_target as isize, final_x, final_y, final_w, final_h
        );

        Ok(())
    }
}

/// 查找包含 SHELLDLL_DefView 的顶层窗口（通常是 WorkerW）
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
        if defview != std::ptr::null_mut() {
            // 检查这个窗口是否是 WorkerW 类
            use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;
            let mut class_name = [0u16; 64];
            let len = GetClassNameW(hwnd, class_name.as_mut_ptr(), 64);
            if len > 0 {
                let name = String::from_utf16_lossy(&class_name[..len as usize]);
                if name == "WorkerW" {
                    FOUND.store(hwnd as isize, Ordering::SeqCst);
                    return 0; // 停止枚举
                }
            }
        }
        1 // 继续枚举
    }

    EnumWindows(Some(enum_cb), 0);
    FOUND.load(Ordering::SeqCst) as HWND
}

/// 查找 0x052C 创建的空 WorkerW 窗口
///
/// 空 WorkerW = 没有 SHELLDLL_DefView 子窗口的 WorkerW
#[cfg(target_os = "windows")]
unsafe fn find_empty_workerw(progman: HWND) -> HWND {
    static FOUND_EMPTY: AtomicIsize = AtomicIsize::new(0);
    FOUND_EMPTY.store(0, Ordering::SeqCst);

    // 存储 progman 的值供回调使用
    static PROGMAN_FOR_CB: AtomicIsize = AtomicIsize::new(0);
    PROGMAN_FOR_CB.store(progman as isize, Ordering::SeqCst);

    unsafe extern "system" fn enum_cb(hwnd: HWND, _: LPARAM) -> BOOL {
        use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;

        // 跳过 Progman 本身
        let pm = PROGMAN_FOR_CB.load(Ordering::SeqCst) as HWND;
        if hwnd == pm {
            return 1;
        }

        let mut class_name = [0u16; 64];
        let len = GetClassNameW(hwnd, class_name.as_mut_ptr(), 64);
        if len > 0 {
            let name = String::from_utf16_lossy(&class_name[..len as usize]);
            if name == "WorkerW" {
                // 检查是否有 SHELLDLL_DefView 子窗口
                let defview = FindWindowExW(
                    hwnd,
                    std::ptr::null_mut(),
                    encode_wide("SHELLDLL_DefView\0").as_ptr(),
                    std::ptr::null(),
                );
                if defview == std::ptr::null_mut() {
                    // 空 WorkerW，就是我们要找的
                    FOUND_EMPTY.store(hwnd as isize, Ordering::SeqCst);
                    return 0; // 停止枚举
                }
            }
        }
        1 // 继续枚举
    }

    EnumWindows(Some(enum_cb), 0);
    let result = FOUND_EMPTY.load(Ordering::SeqCst) as HWND;

    if result != std::ptr::null_mut() {
        println!(
            "[DesktopEmbedder] Found empty WorkerW: 0x{:X}",
            result as isize
        );
    } else {
        println!("[DesktopEmbedder] No empty WorkerW found");
    }

    result
}

/// 启动 Z-order 监控定时器
///
/// 每 500ms 检查一次所有嵌入窗口的位置和可见性。
/// 对于 Mode C（壁纸在 DefView 之上），确保壁纸始终在最顶层。
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

        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));

            let windows = {
                let guard = EMBEDDED_WINDOWS.lock().unwrap();
                match guard.as_ref() {
                    Some(list) if !list.is_empty() => list.clone(),
                    _ => {
                        ZORDER_TIMER_RUNNING.store(false, Ordering::SeqCst);
                        println!(
                            "[DesktopEmbedder] Z-order monitor timer stopped (no embedded windows)"
                        );
                        return;
                    }
                }
            };

            let mode = EMBED_MODE.load(Ordering::SeqCst);
            if mode == 3 {
                // Mode C：确保壁纸在 DefView 之上
                unsafe {
                    use windows_sys::Win32::UI::WindowsAndMessaging::{
                        HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
                    };
                    for ew in &windows {
                        SetWindowPos(
                            ew.hwnd as HWND,
                            HWND_TOP,
                            0,
                            0,
                            0,
                            0,
                            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                        );
                    }
                }
            }
            // Mode A 和 B 不需要定时修正
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
                hwnd,
                list.len()
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
