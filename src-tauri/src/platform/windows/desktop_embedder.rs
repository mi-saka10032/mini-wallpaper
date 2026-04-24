//! Win32 桌面壁纸嵌入模块
//!
//! 核心原理：
//! 1. FindWindow("Progman") 找到桌面窗口
//! 2. SendMessageTimeout(Progman, 0x052C) 触发 explorer 创建 WorkerW
//! 3. 根据 Windows 版本选择不同的 WorkerW 查找策略：
//!    - 24H2+：WorkerW 是 Progman 的子窗口，直接 FindWindowExW(progman, "WorkerW")
//!    - 旧版本：WorkerW 是顶层窗口，通过 EnumWindows 找到 SHELLDLL_DefView 所在
//!      WorkerW 的前一个兄弟
//! 4. SetParent(tauri_hwnd, workerw) 将壁纸窗口嵌入桌面层级
//!
//! 版本兼容策略：
//! - 24H2+（Build >= 26100）：SetParent 后 DWM 会通过 WM_NCCALCSIZE 注入隐藏 NC 边框，
//!   需要 WndProc 子类化拦截 + 样式清理 + NC 补偿 fallback
//! - 旧版本（< 24H2）：经典嵌入方案，无需额外修复

#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, SendMessageTimeoutW, SetParent, SMTO_NORMAL,
};

#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicIsize, Ordering};

// ===== 方案 A：WndProc 子类化 =====
//
// 通过 SetWindowLongPtrW(GWL_WNDPROC) 替换窗口过程，
// 拦截 WM_NCCALCSIZE 消息，强制将 NC 区域设为 0。
// 这样无论 24H2 的 DWM 如何尝试注入边框，都会被我们的 WndProc 拦截。
//
// 原始 WndProc 保存在全局变量中，其他消息正常转发。

/// 保存原始 WndProc 的全局变量（每个嵌入窗口一个）
/// 使用简单的静态数组支持最多 8 个显示器
#[cfg(target_os = "windows")]
static ORIGINAL_WNDPROCS: [AtomicIsize; 8] = [
    AtomicIsize::new(0),
    AtomicIsize::new(0),
    AtomicIsize::new(0),
    AtomicIsize::new(0),
    AtomicIsize::new(0),
    AtomicIsize::new(0),
    AtomicIsize::new(0),
    AtomicIsize::new(0),
];

/// 保存对应的 HWND，用于在 WndProc 中查找正确的原始 WndProc
#[cfg(target_os = "windows")]
static SUBCLASSED_HWNDS: [AtomicIsize; 8] = [
    AtomicIsize::new(0),
    AtomicIsize::new(0),
    AtomicIsize::new(0),
    AtomicIsize::new(0),
    AtomicIsize::new(0),
    AtomicIsize::new(0),
    AtomicIsize::new(0),
    AtomicIsize::new(0),
];

/// 查找 HWND 对应的原始 WndProc
#[cfg(target_os = "windows")]
fn find_original_wndproc(hwnd: HWND) -> Option<isize> {
    let hwnd_val = hwnd as isize;
    for i in 0..8 {
        if SUBCLASSED_HWNDS[i].load(Ordering::SeqCst) == hwnd_val {
            let proc = ORIGINAL_WNDPROCS[i].load(Ordering::SeqCst);
            if proc != 0 {
                return Some(proc);
            }
        }
    }
    None
}

/// 注册子类化信息
#[cfg(target_os = "windows")]
fn register_subclass(hwnd: HWND, original_proc: isize) {
    let hwnd_val = hwnd as isize;
    for i in 0..8 {
        // 找到空槽位或已存在的同一 HWND
        let existing = SUBCLASSED_HWNDS[i].load(Ordering::SeqCst);
        if existing == 0 || existing == hwnd_val {
            SUBCLASSED_HWNDS[i].store(hwnd_val, Ordering::SeqCst);
            ORIGINAL_WNDPROCS[i].store(original_proc, Ordering::SeqCst);
            println!(
                "[DesktopEmbedder] Subclass registered: slot={}, hwnd=0x{:X}, orig_proc=0x{:X}",
                i, hwnd_val, original_proc
            );
            return;
        }
    }
    println!(
        "[DesktopEmbedder] WARNING: No free subclass slot for hwnd=0x{:X}",
        hwnd_val
    );
}

/// 子类化的窗口过程
///
/// 核心：拦截 WM_NCCALCSIZE，强制返回 0（即 NC 区域为空），
/// 其他消息转发给原始 WndProc
#[cfg(target_os = "windows")]
unsafe extern "system" fn subclass_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{CallWindowProcW, WM_NCCALCSIZE};

    if msg == WM_NCCALCSIZE {
        // 当 wParam == TRUE (1) 时，lparam 指向 NCCALCSIZE_PARAMS 结构体
        // 我们不修改它，直接返回 0，告诉系统"不需要任何非客户区"
        //
        // 当 wParam == FALSE (0) 时，lparam 指向 RECT
        // 同样返回 0，表示整个窗口矩形都是客户区
        //
        // 这是阻止 24H2 DWM 注入隐藏边框的关键
        return 0;
    }

    // 其他消息转发给原始 WndProc
    if let Some(original_proc) = find_original_wndproc(hwnd) {
        // 将保存的 isize 还原为函数指针，再包装为 WNDPROC (= Option<fn(...)>)
        type WndProcFn = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;
        let fn_ptr: WndProcFn = std::mem::transmute(original_proc as usize);
        CallWindowProcW(Some(fn_ptr), hwnd, msg, wparam, lparam)
    } else {
        // 找不到原始 WndProc，使用 DefWindowProcW 兜底
        windows_sys::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

/// 检测当前 Windows 版本是否为 24H2 或更高版本
///
/// Windows 11 24H2 的 Build Number 为 26100。
/// 24H2 改变了桌面窗口层级结构（WorkerW 变为 Progman 子窗口），
/// 并且 SetParent 后会注入隐藏的 NC 边框，需要特殊处理。
#[cfg(target_os = "windows")]
fn is_win11_24h2_or_later() -> bool {
    use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;

    unsafe {
        let mut osvi: OSVERSIONINFOW = std::mem::zeroed();
        osvi.dwOSVersionInfoSize = std::mem::size_of::<OSVERSIONINFOW>() as u32;

        // GetVersionExW 在 Win8.1+ 会被 manifest 限制，但对于 Build Number 检测
        // 我们使用 RtlGetVersion 作为更可靠的替代方案
        type RtlGetVersionFn = unsafe extern "system" fn(*mut OSVERSIONINFOW) -> i32;

        let ntdll = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(
            encode_wide("ntdll.dll\0").as_ptr(),
        );
        if ntdll == std::ptr::null_mut() {
            println!("[DesktopEmbedder] WARNING: Failed to get ntdll.dll handle, assuming legacy path");
            return false;
        }

        let proc = windows_sys::Win32::System::LibraryLoader::GetProcAddress(
            ntdll,
            b"RtlGetVersion\0".as_ptr(),
        );
        if proc.is_none() {
            println!("[DesktopEmbedder] WARNING: Failed to get RtlGetVersion, assuming legacy path");
            return false;
        }

        let rtl_get_version: RtlGetVersionFn = std::mem::transmute(proc);
        let status = rtl_get_version(&mut osvi as *mut OSVERSIONINFOW);

        if status == 0 {
            // STATUS_SUCCESS
            let build = osvi.dwBuildNumber;
            let is_24h2 = build >= 26100;
            println!(
                "[DesktopEmbedder] Windows version: {}.{}.{} → {}",
                osvi.dwMajorVersion,
                osvi.dwMinorVersion,
                build,
                if is_24h2 { "24H2+ (new path)" } else { "Legacy (classic path)" }
            );
            is_24h2
        } else {
            println!("[DesktopEmbedder] WARNING: RtlGetVersion failed (status={}), assuming legacy path", status);
            false
        }
    }
}

/// 在 Windows 上查找 WorkerW 窗口并将指定 HWND 嵌入桌面层级
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
    use std::mem::zeroed;

    use windows_sys::Win32::{
        Graphics::Gdi::ClientToScreen,
        UI::WindowsAndMessaging::{
            CallWindowProcW, GetClientRect, GetWindowLongPtrW, GetWindowRect, MoveWindow,
            SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, GWL_STYLE,
            GWL_WNDPROC, LWA_ALPHA, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
            SWP_NOZORDER, WS_BORDER, WS_CAPTION, WS_CHILD, WS_DLGFRAME, WS_EX_CLIENTEDGE,
            WS_EX_DLGMODALFRAME, WS_EX_LAYERED, WS_EX_STATICEDGE, WS_EX_TRANSPARENT,
            WS_EX_WINDOWEDGE, WS_THICKFRAME, WS_VISIBLE,
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

        // 3. 检测 Windows 版本，决定嵌入策略
        let is_24h2 = is_win11_24h2_or_later();

        // 4. 根据版本选择 WorkerW 查找策略
        //    - 24H2+：WorkerW 是 Progman 的子窗口，直接 FindWindowExW(progman, "WorkerW")
        //    - 旧版本：WorkerW 是顶层窗口，通过 EnumWindows 经典方案查找
        let workerw = if is_24h2 {
            let w = FindWindowExW(
                progman,
                std::ptr::null_mut(),
                encode_wide("WorkerW\0").as_ptr(),
                std::ptr::null(),
            );
            println!("[DesktopEmbedder] 24H2+ path: FindWindowExW(Progman, WorkerW) → {:?}", w);
            if w == std::ptr::null_mut() {
                // 24H2 路径失败，fallback 到经典方案
                println!("[DesktopEmbedder] 24H2+ path failed, falling back to classic EnumWindows");
                find_workerw().map_err(|e| format!("Both 24H2+ and classic WorkerW search failed: {}", e))?
            } else {
                w
            }
        } else {
            println!("[DesktopEmbedder] Legacy path: using EnumWindows to find WorkerW");
            find_workerw()?
        };

        // 使用前端传入的显示器坐标和尺寸（单个显示器的物理分辨率）
        let target_x = monitor_x;
        let target_y = monitor_y;
        let target_w = monitor_width;
        let target_h = monitor_height;

        println!(
            "[DesktopEmbedder] Monitor rect: ({}, {}) {}x{}",
            target_x, target_y, target_w, target_h
        );

        // ===== 5. 根据版本执行不同的嵌入策略 =====
        if is_24h2 {
            // ===== 24H2+ 路径：WndProc 子类化 + 样式清理 + NC 补偿 =====
            //
            // Windows 11 24H2 在 SetParent 后会通过 WM_NCCALCSIZE 注入隐藏的 NC 边框。
            //
            // 核心思路：
            // 1. 在 SetParent 之前子类化窗口过程（替换 WndProc）
            // 2. 新的 WndProc 拦截所有 WM_NCCALCSIZE 消息，强制返回 0
            // 3. 这样当 SetParent 内部触发 WM_NCCALCSIZE 时，我们的 WndProc 会
            //    告诉系统"不需要任何非客户区"，从根源阻止 NC 边框注入
            // 4. 其他消息正常转发给原始 WndProc，不影响 Tauri/WebView 的正常功能

            // 5a. 子类化窗口过程：保存原始 WndProc，替换为我们的 subclass_wndproc
            let original_proc = GetWindowLongPtrW(hwnd as HWND, GWL_WNDPROC);
            if original_proc != 0 {
                register_subclass(hwnd as HWND, original_proc);
                let new_proc = subclass_wndproc as isize;
                SetWindowLongPtrW(hwnd as HWND, GWL_WNDPROC, new_proc);
                println!(
                    "[DesktopEmbedder] WndProc subclassed: orig=0x{:X}, new=0x{:X}",
                    original_proc, new_proc
                );
            } else {
                println!("[DesktopEmbedder] WARNING: GetWindowLongPtrW(GWL_WNDPROC) returned 0, skipping subclass");
            }

            // 5b. 清除窗口样式中的所有边框位
            let style = GetWindowLongPtrW(hwnd as HWND, GWL_STYLE);
            let clean_style = (style
                & !(WS_CAPTION as isize)
                & !(WS_THICKFRAME as isize)
                & !(WS_BORDER as isize)
                & !(WS_DLGFRAME as isize))
                | WS_CHILD as isize
                | WS_VISIBLE as isize;
            SetWindowLongPtrW(hwnd as HWND, GWL_STYLE, clean_style);

            // 5c. 设置扩展样式：WS_EX_LAYERED + WS_EX_TRANSPARENT（鼠标穿透）
            //     清除所有可能的边框扩展样式
            let ex_style = GetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE);
            let clean_ex = (ex_style
                & !(WS_EX_CLIENTEDGE as isize)
                & !(WS_EX_STATICEDGE as isize)
                & !(WS_EX_WINDOWEDGE as isize)
                & !(WS_EX_DLGMODALFRAME as isize))
                | WS_EX_LAYERED as isize
                | WS_EX_TRANSPARENT as isize;
            SetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE, clean_ex);

            // 5d. 设置 Layered 窗口属性：完全不透明
            SetLayeredWindowAttributes(hwnd as HWND, 0, 0xFF, LWA_ALPHA);

            // 5e. SWP_FRAMECHANGED 强制系统重新发送 WM_NCCALCSIZE
            //     此时我们的 subclass_wndproc 已经就位，会拦截并返回 0
            SetWindowPos(
                hwnd as HWND,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE,
            );

            println!("[DesktopEmbedder] 24H2+: WndProc subclassed, styles cleaned, SWP_FRAMECHANGED sent");

            // 5f. SetParent 嵌入
            let prev_parent = SetParent(hwnd as HWND, workerw);
            if prev_parent == std::ptr::null_mut() {
                return Err("SetParent failed".into());
            }

            // 5g. SetParent 后再次 SWP_FRAMECHANGED，确保 NC 区域在新父窗口下也被正确清零
            SetWindowPos(
                hwnd as HWND,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE,
            );

            // 5h. 定位窗口到目标显示器区域
            MoveWindow(hwnd as HWND, target_x, target_y, target_w, target_h, 1);

            // 5i. 验证：测量实际 NC 偏移，输出日志
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
                "[DesktopEmbedder] 24H2+ Verify: NC edges L={} T={} R={} B={}, client={}x{}, target={}x{}",
                nc_left, nc_top, nc_right, nc_bottom, client_w, client_h, target_w, target_h
            );

            if nc_left != 0 || nc_top != 0 || nc_right != 0 || nc_bottom != 0 {
                println!(
                    "[DesktopEmbedder] WARNING: NC offset still present! L={} T={} R={} B={}",
                    nc_left, nc_top, nc_right, nc_bottom
                );
                // 回退到 NC 补偿
                let comp_x = target_x - nc_left;
                let comp_y = target_y - nc_top;
                let comp_w = target_w + nc_left + nc_right;
                let comp_h = target_h + nc_top + nc_bottom;
                MoveWindow(hwnd as HWND, comp_x, comp_y, comp_w, comp_h, 1);
                println!(
                    "[DesktopEmbedder] Fallback: NC compensation → pos=({}, {}), size={}x{}",
                    comp_x, comp_y, comp_w, comp_h
                );
            } else {
                println!("[DesktopEmbedder] 24H2+ Plan A success: WM_NCCALCSIZE interception eliminated NC offset!");
            }
        } else {
            // ===== 旧版本路径：经典嵌入，无需 NC 修复 =====
            //
            // 24H2 之前的 Windows 版本不会在 SetParent 后注入 NC 边框，
            // 也不存在 DWM 强制圆角裁剪问题（Win10）或圆角可正常禁用（Win11 早期）。
            // 只需基础的 SetParent + MoveWindow 即可。

            // 设置鼠标穿透（WS_EX_TRANSPARENT）
            let ex_style = GetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE);
            SetWindowLongPtrW(
                hwnd as HWND,
                GWL_EXSTYLE,
                ex_style | WS_EX_TRANSPARENT as isize,
            );

            let prev_parent = SetParent(hwnd as HWND, workerw);
            if prev_parent == std::ptr::null_mut() {
                return Err("SetParent failed".into());
            }

            MoveWindow(hwnd as HWND, target_x, target_y, target_w, target_h, 1);
            println!("[DesktopEmbedder] Legacy path: simple SetParent + MoveWindow completed");
        }

        // 注意：DWM 圆角裁剪（~1px）已确认无法通过任何窗口级 API 消除，
        // Overscan 方案虽能消除圆角但会导致多屏交接处溢出更明显（色差壁纸下尤为突出）。
        // 权衡后选择接受 1px 圆角溢出——面积更小、更可控。

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
