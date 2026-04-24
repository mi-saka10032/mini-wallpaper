//! Win32 桌面壁纸嵌入模块 — Direct Child 方案
//!
//! 基于博主的 Direct Child 方案实现，核心原理：
//!
//! 1. FindWindow("Progman") 找到桌面窗口
//! 2. RaiseDesktop: 发送多次 0x052C 消息确保壁纸层级就绪
//! 3. 查找 SHELLDLL_DefView 的位置：
//!    - 24H2：DefView 在 Progman 中 → 壁纸 SetParent 到 Progman
//!    - 23H2 回退：DefView 在 WorkerW 中 → 壁纸 SetParent 到该 WorkerW 或 Progman
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
    FindWindowExW, FindWindowW, GetClassNameW, GetWindow, MoveWindow,
    SendMessageTimeoutW, SetParent, SetWindowLongPtrW, SetWindowPos, ShowWindow, SMTO_NORMAL,
};

#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
#[cfg(target_os = "windows")]
use std::sync::Mutex;

// ===== 全局状态 =====

/// 已嵌入的壁纸窗口列表
#[cfg(target_os = "windows")]
static EMBEDDED_WINDOWS: Mutex<Option<Vec<EmbeddedWindow>>> = Mutex::new(None);

/// Z-order 监控定时器是否已启动
#[cfg(target_os = "windows")]
static ZORDER_TIMER_RUNNING: AtomicBool = AtomicBool::new(false);

/// 缓存的 SHELLDLL_DefView 句柄
#[cfg(target_os = "windows")]
static CACHED_DEFVIEW: AtomicIsize = AtomicIsize::new(0);

/// 缓存的 WorkerW 句柄（系统壁纸窗口）
#[cfg(target_os = "windows")]
static CACHED_WORKERW: AtomicIsize = AtomicIsize::new(0);

/// 是否为 23H2 回退模式（DefView 不在 Progman 中）
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

/// 发送 0x052C 系列消息，确保桌面壁纸层级就绪
///
/// 参考 ExplorerPatcher: https://github.com/valinet/ExplorerPatcher/issues/525
#[cfg(target_os = "windows")]
unsafe fn raise_desktop(progman: HWND) -> bool {
    let mut res0: usize = usize::MAX;
    let mut res1: usize = usize::MAX;
    let mut res2: usize = usize::MAX;
    let mut res3: usize = usize::MAX;

    // 检查壁纸是否已初始化
    SendMessageTimeoutW(
        progman, 0x052C, 0xA, 0, SMTO_NORMAL, 1000,
        &mut res0 as *mut usize,
    );
    if res0 != 0 {
        println!("[DesktopEmbedder] RaiseDesktop: wallpaper not initialized (res0={})", res0);
        // 不直接返回失败，继续尝试
    }

    // 准备生成壁纸窗口
    SendMessageTimeoutW(
        progman, 0x052C, 0xD, 0, SMTO_NORMAL, 1000,
        &mut res1 as *mut usize,
    );
    SendMessageTimeoutW(
        progman, 0x052C, 0xD, 1, SMTO_NORMAL, 1000,
        &mut res2 as *mut usize,
    );
    // "Animate desktop" - 确保壁纸窗口存在
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

/// 检查窗口是否为 WorkerW 类（桌面层级中的 WorkerW 几乎肯定属于 explorer）
#[cfg(target_os = "windows")]
unsafe fn is_explorer_worker(hwnd: HWND) -> bool {
    if hwnd == std::ptr::null_mut() {
        return false;
    }
    let mut class_name = [0u16; 256];
    let len = GetClassNameW(hwnd, class_name.as_mut_ptr(), 256);
    if len <= 0 {
        return false;
    }
    let name = String::from_utf16_lossy(&class_name[..len as usize]);
    name == "WorkerW" || name == "WorkerA"
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
    if prev == defview {
        // 顺序已正确
        return (true, std::ptr::null_mut());
    }

    // 修正 Z-order：将壁纸放到 DefView 之后
    SetWindowPos(
        embed_wnd, defview, 0, 0, 0, 0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
    );

    if is_explorer_worker(prev) {
        return (true, std::ptr::null_mut());
    }

    (false, prev)
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
    use windows_sys::Win32::Graphics::Dwm::{
        DwmEnableBlurBehindWindow, DwmExtendFrameIntoClientArea,
        DWM_BLURBEHIND, MARGINS,
    };
    use windows_sys::Win32::Graphics::Gdi::CreateRectRgn;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetLayeredWindowAttributes, GWL_EXSTYLE, GWL_STYLE,
        GW_HWNDPREV, HWND_BOTTOM, HWND_TOP, LWA_ALPHA,
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
            encode_wide("Progman\0").as_ptr(),
            encode_wide("Program Manager\0").as_ptr(),
        );
        if progman == std::ptr::null_mut() {
            return Err("Failed to find Progman window".into());
        }
        println!("[DesktopEmbedder] Found Progman: 0x{:X}", progman as isize);

        // ===== 2. RaiseDesktop =====
        if !raise_desktop(progman) {
            println!("[DesktopEmbedder] WARNING: RaiseDesktop returned false, continuing anyway...");
        }

        // 短暂等待 explorer 处理
        std::thread::sleep(std::time::Duration::from_millis(150));

        // ===== 3. 查找 SHELLDLL_DefView =====
        //
        // 24H2: DefView 在 Progman 中
        // 23H2: DefView 可能在 Progman 的前序兄弟 WorkerW 中
        let mut defview = FindWindowExW(
            progman,
            std::ptr::null_mut(),
            encode_wide("SHELLDLL_DefView\0").as_ptr(),
            encode_wide("\0").as_ptr(),
        );

        let mut worker1: HWND = std::ptr::null_mut(); // 包含 DefView 的 WorkerW
        let mut worker2: HWND = std::ptr::null_mut(); // 额外的 WorkerW
        let mut is_legacy = false;

        if defview == std::ptr::null_mut() {
            // 24H2 没找到，回退 23H2 搜索模式
            println!("[DesktopEmbedder] DefView not in Progman, trying 23H2 fallback...");

            let worker_p1 = GetWindow(progman, GW_HWNDPREV);
            if worker_p1 != std::ptr::null_mut() {
                defview = FindWindowExW(
                    worker_p1,
                    std::ptr::null_mut(),
                    encode_wide("SHELLDLL_DefView\0").as_ptr(),
                    encode_wide("\0").as_ptr(),
                );

                if defview != std::ptr::null_mut() {
                    worker1 = worker_p1;
                    println!(
                        "[DesktopEmbedder] 23H2 mode: DefView in WorkerW 0x{:X}",
                        worker1 as isize
                    );
                } else {
                    worker2 = worker_p1;
                    let worker_p2 = GetWindow(worker_p1, GW_HWNDPREV);
                    if worker_p2 != std::ptr::null_mut() {
                        defview = FindWindowExW(
                            worker_p2,
                            std::ptr::null_mut(),
                            encode_wide("SHELLDLL_DefView\0").as_ptr(),
                            encode_wide("\0").as_ptr(),
                        );
                        if defview != std::ptr::null_mut() {
                            worker1 = worker_p2;
                            println!(
                                "[DesktopEmbedder] 23H2 mode (2nd prev): DefView in WorkerW 0x{:X}",
                                worker1 as isize
                            );
                        }
                    }
                }
            }

            if defview == std::ptr::null_mut() {
                return Err("Failed to find SHELLDLL_DefView".into());
            }
            is_legacy = true;
        } else {
            println!(
                "[DesktopEmbedder] 24H2 mode: DefView 0x{:X} in Progman",
                defview as isize
            );
        }

        IS_LEGACY_MODE.store(is_legacy, Ordering::SeqCst);
        CACHED_DEFVIEW.store(defview as isize, Ordering::SeqCst);

        // ===== 4. 找到 Progman 内部的 WorkerW（系统壁纸窗口） =====
        let mut worker_in_progman = FindWindowExW(
            progman,
            std::ptr::null_mut(),
            encode_wide("WorkerW\0").as_ptr(),
            encode_wide("\0").as_ptr(),
        );
        if worker_in_progman == std::ptr::null_mut() {
            worker_in_progman = FindWindowExW(
                progman,
                std::ptr::null_mut(),
                encode_wide("WorkerA\0").as_ptr(),
                encode_wide("\0").as_ptr(),
            );
        }

        // 23H2 回退
        let effective_worker = if worker_in_progman == std::ptr::null_mut() {
            if worker2 != std::ptr::null_mut() {
                worker2
            } else {
                progman // 最终回退
            }
        } else {
            worker_in_progman
        };

        CACHED_WORKERW.store(effective_worker as isize, Ordering::SeqCst);
        println!(
            "[DesktopEmbedder] WorkerW (wallpaper): 0x{:X}, is_legacy={}",
            effective_worker as isize, is_legacy
        );

        // ===== 5. 准备窗口样式 =====
        let mut style = GetWindowLongPtrW(hwnd as HWND, GWL_STYLE);
        let mut ex_style = GetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE);

        // 添加 WS_EX_LAYERED
        ex_style |= WS_EX_LAYERED as isize;
        // 移除 WS_EX_TOOLWINDOW
        ex_style &= !(WS_EX_TOOLWINDOW as isize);
        // 移除不需要的样式
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

        // ===== 6. DWM 透明 hack =====
        // 先 SetParent(NULL) 确保是顶级窗口（DWM 要求）
        SetParent(hwnd as HWND, std::ptr::null_mut());

        // DwmExtendFrameIntoClientArea - 扩展框架到客户区
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

        // DwmEnableBlurBehindWindow - 启用模糊背景（实现透明）
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

        // SetLayeredWindowAttributes - 设置完全不透明
        SetLayeredWindowAttributes(hwnd as HWND, 0, 0xFF, LWA_ALPHA);
        println!("[DesktopEmbedder] SetLayeredWindowAttributes: alpha=0xFF");

        // ===== 7. SetParent 到目标窗口 =====
        let embed_target = if is_legacy {
            if effective_worker != progman { effective_worker } else { progman }
        } else {
            progman
        };

        let prev_parent = SetParent(hwnd as HWND, embed_target);
        if prev_parent == std::ptr::null_mut() {
            // SetParent 返回 NULL 可能表示失败，但也可能是之前没有父窗口
            println!("[DesktopEmbedder] WARNING: SetParent returned NULL");
        }
        println!(
            "[DesktopEmbedder] SetParent: HWND 0x{:X} -> 0x{:X}",
            hwnd, embed_target as isize
        );

        // ===== 8. Z-order 管理 =====
        // 壁纸 → HWND_TOP
        SetWindowPos(
            hwnd as HWND, HWND_TOP, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_DRAWFRAME,
        );
        // DefView → HWND_TOP（覆盖在壁纸之上）
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

        // ===== 9. 定位窗口到目标显示器 =====
        // 使用 AdjustWindowRect 的等效逻辑
        MoveWindow(
            hwnd as HWND,
            monitor_x, monitor_y,
            monitor_width, monitor_height,
            1,
        );
        println!(
            "[DesktopEmbedder] MoveWindow: pos=({},{}), size={}x{}",
            monitor_x, monitor_y, monitor_width, monitor_height
        );

        // ===== 10. 显示窗口 =====
        ShowWindow(progman, SW_SHOW);
        ShowWindow(hwnd as HWND, SW_SHOW);
        ShowWindow(effective_worker, SW_SHOW);

        // ===== 11. 注册到全局列表并启动 Z-order 监控 =====
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

        // 24H2 模式下启动 Z-order 监控定时器
        if !is_legacy {
            start_zorder_timer();
        }

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
                        println!(
                            "[DesktopEmbedder] Z-order timer stopped (no embedded windows)"
                        );
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
                    let (ok, conflict) =
                        ensure_embed_below_defview(defview, ew.hwnd as HWND);

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
                                "[DesktopEmbedder] ERROR: Repeated Z-order conflict detected! Stopping monitor."
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
    CACHED_DEFVIEW.store(0, Ordering::SeqCst);
    CACHED_WORKERW.store(0, Ordering::SeqCst);
    IS_LEGACY_MODE.store(false, Ordering::SeqCst);
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