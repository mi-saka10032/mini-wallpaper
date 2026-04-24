//! Win32 桌面壁纸嵌入模块
//!
//! 核心原理：
//! 1. FindWindow("Progman") 找到桌面窗口
//! 2. SendMessageTimeout(Progman, 0x052C) 触发 explorer 创建 WorkerW
//! 3. EnumWindows 找到正确的 WorkerW（包含 SHELLDLL_DefView 子窗口的那个的下一个兄弟）
//! 4. SetParent(tauri_hwnd, workerw) 将壁纸窗口嵌入桌面层级
//!
//! 24H2 修复（方案 A）：
//! Windows 11 24H2 在 SetParent 后会通过 WM_NCCALCSIZE 注入隐藏的 NC 边框（~8px），
//! 导致壁纸窗口出现偏移和黏着。方案 A 通过子类化窗口过程（WndProc Subclass），
//! 拦截 WM_NCCALCSIZE 消息并强制返回 0 NC 区域，从根源消除偏移。

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
    AtomicIsize::new(0), AtomicIsize::new(0),
    AtomicIsize::new(0), AtomicIsize::new(0),
    AtomicIsize::new(0), AtomicIsize::new(0),
    AtomicIsize::new(0), AtomicIsize::new(0),
];

/// 保存对应的 HWND，用于在 WndProc 中查找正确的原始 WndProc
#[cfg(target_os = "windows")]
static SUBCLASSED_HWNDS: [AtomicIsize; 8] = [
    AtomicIsize::new(0), AtomicIsize::new(0),
    AtomicIsize::new(0), AtomicIsize::new(0),
    AtomicIsize::new(0), AtomicIsize::new(0),
    AtomicIsize::new(0), AtomicIsize::new(0),
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
            println!("[DesktopEmbedder] Subclass registered: slot={}, hwnd=0x{:X}, orig_proc=0x{:X}",
                i, hwnd_val, original_proc);
            return;
        }
    }
    println!("[DesktopEmbedder] WARNING: No free subclass slot for hwnd=0x{:X}", hwnd_val);
}

/// 子类化的窗口过程
///
/// 核心：拦截 WM_NCCALCSIZE / WM_NCPAINT / WM_NCHITTEST，
/// 彻底阻止 24H2 DWM 注入任何 NC 区域装饰（边框、圆角等），
/// 其他消息转发给原始 WndProc
#[cfg(target_os = "windows")]
unsafe extern "system" fn subclass_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, WM_NCCALCSIZE, WM_NCPAINT, WM_NCHITTEST, WM_ERASEBKGND, HTCLIENT,
    };

    match msg {
        WM_NCCALCSIZE => {
            // 当 wParam == TRUE (1) 时，lparam 指向 NCCALCSIZE_PARAMS 结构体
            // 当 wParam == FALSE (0) 时，lparam 指向 RECT
            // 直接返回 0，告诉系统"不需要任何非客户区"
            // 这是阻止 24H2 DWM 注入隐藏边框的关键
            return 0;
        }
        WM_NCPAINT => {
            // 阻止系统绘制任何非客户区内容（边框、圆角装饰等）
            // 24H2 的 DWM 可能通过 WM_NCPAINT 在窗口边缘绘制 1px 的圆角/边框线
            // 直接返回 0，跳过所有 NC 绘制
            return 0;
        }
        WM_NCHITTEST => {
            // 强制整个窗口区域都被视为客户区（HTCLIENT）
            // 防止系统在窗口边缘检测到 NC 区域（如边框、标题栏）
            // 从而避免 DWM 对这些区域进行特殊渲染
            return HTCLIENT as LRESULT;
        }
        WM_ERASEBKGND => {
            // 用黑色画刷填充窗口背景
            // 当使用 overscan（窗口比显示器大 2px）消除 DWM 圆角时，
            // 溢出区域如果不填充会显示为白色（默认窗口背景色）。
            // 黑色在 DWM 合成透明窗口时会被视为透明区域，
            // 即使不完全透明，黑色在桌面边缘也远比白色不显眼。
            use windows_sys::Win32::Graphics::Gdi::{
                FillRect, GetStockObject, BLACK_BRUSH,
            };
            use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;
            let hdc = wparam as windows_sys::Win32::Graphics::Gdi::HDC;
            let mut rc: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rc);
            FillRect(hdc, &rc, GetStockObject(BLACK_BRUSH as i32) as _);
            return 1; // 返回非零值表示已处理背景擦除
        }
        _ => {}
    }

    // 其他消息转发给原始 WndProc
    if let Some(original_proc) = find_original_wndproc(hwnd) {
        // 将保存的 isize 还原为函数指针，再包装为 WNDPROC (= Option<fn(...)>)
        type WndProcFn = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;
        let fn_ptr: WndProcFn = std::mem::transmute(original_proc as usize);
        CallWindowProcW(
            Some(fn_ptr),
            hwnd,
            msg,
            wparam,
            lparam,
        )
    } else {
        // 找不到原始 WndProc，使用 DefWindowProcW 兜底
        windows_sys::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

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
            CallWindowProcW, GetClientRect, GetWindowLongPtrW, GetWindowRect,
            MoveWindow, SetWindowLongPtrW, SetWindowPos, SetLayeredWindowAttributes,
            GWL_EXSTYLE, GWL_STYLE, GWL_WNDPROC, LWA_ALPHA,
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

        // ===== 3. 方案 A：子类化 WndProc 拦截 WM_NCCALCSIZE =====
        //
        // Windows 11 24H2 在 SetParent 后会通过 WM_NCCALCSIZE 注入隐藏的 NC 边框。
        // 方案 B（Layered 前置）已验证无效——DWM 的 NC 注入发生在 SetParent 内部，
        // 与窗口是否预先标记为 Layered 无关。
        //
        // 方案 A 的核心思路：
        // 1. 在 SetParent 之前子类化窗口过程（替换 WndProc）
        // 2. 新的 WndProc 拦截所有 WM_NCCALCSIZE 消息，强制返回 0
        // 3. 这样当 SetParent 内部触发 WM_NCCALCSIZE 时，我们的 WndProc 会
        //    告诉系统"不需要任何非客户区"，从根源阻止 NC 边框注入
        // 4. 其他消息正常转发给原始 WndProc，不影响 Tauri/WebView 的正常功能

        // 3a. 子类化窗口过程：保存原始 WndProc，替换为我们的 subclass_wndproc
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

        // 3b. 清除窗口样式中的所有边框位
        let style = GetWindowLongPtrW(hwnd as HWND, GWL_STYLE);
        let clean_style = (style
            & !(WS_CAPTION as isize)
            & !(WS_THICKFRAME as isize)
            & !(WS_BORDER as isize)
            & !(WS_DLGFRAME as isize))
            | WS_CHILD as isize
            | WS_VISIBLE as isize;
        SetWindowLongPtrW(hwnd as HWND, GWL_STYLE, clean_style);

        // 3c. 设置扩展样式：WS_EX_LAYERED + WS_EX_TRANSPARENT（鼠标穿透）
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

        // 3d. 设置 Layered 窗口属性：完全不透明
        SetLayeredWindowAttributes(hwnd as HWND, 0, 0xFF, LWA_ALPHA);

        // 3e. SWP_FRAMECHANGED 强制系统重新发送 WM_NCCALCSIZE
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

        println!("[DesktopEmbedder] Pre-SetParent: WndProc subclassed, styles cleaned, SWP_FRAMECHANGED sent");

        // ===== 3f. 在 SetParent 之前禁用 DWM 圆角 =====
        //     DWMWA_WINDOW_CORNER_PREFERENCE 只对顶层窗口生效，
        //     SetParent 后窗口变为子窗口就无法设置了。
        //     所以必须在 SetParent 之前调用。
        let _corner_disabled_ok;
        {
            use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;

            // DWMWA_WINDOW_CORNER_PREFERENCE = 33 (Win11 22H2+)
            // DWMWCP_DONOTROUND = 1
            // 强制窗口使用直角，不绘制圆角
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
                _corner_disabled_ok = true;
                println!("[DesktopEmbedder] DWM: Window corner preference set to DONOTROUND (pre-SetParent)");
            } else {
                _corner_disabled_ok = false;
                println!("[DesktopEmbedder] DWM: DWMWA_WINDOW_CORNER_PREFERENCE failed (hr=0x{:X}), will use overscan fallback", hr);
            }

            // DWMWA_NCRENDERING_POLICY = 2, DWMNCRP_DISABLED = 1
            // 也在 SetParent 前设置一次
            const DWMWA_NCRENDERING_POLICY: u32 = 2;
            const DWMNCRP_DISABLED: u32 = 1;
            let ncrp: u32 = DWMNCRP_DISABLED;
            DwmSetWindowAttribute(
                hwnd as HWND,
                DWMWA_NCRENDERING_POLICY,
                &ncrp as *const u32 as *const _,
                std::mem::size_of::<u32>() as u32,
            );
            println!("[DesktopEmbedder] DWM: NC rendering policy set to DISABLED (pre-SetParent)");
        }

        // ===== 4. SetParent 嵌入 =====
        //     SetParent 内部会触发 WM_NCCALCSIZE，但我们的 subclass_wndproc
        //     会拦截它并返回 0，阻止 NC 边框注入
        let prev_parent = SetParent(hwnd as HWND, workerw);
        if prev_parent == std::ptr::null_mut() {
            return Err("SetParent failed".into());
        }

        // 4a. SetParent 后再次设置 DWM 属性（NC 渲染策略可能被 SetParent 重置）
        {
            use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;

            const DWMWA_NCRENDERING_POLICY: u32 = 2;
            const DWMNCRP_DISABLED: u32 = 1;
            let ncrp: u32 = DWMNCRP_DISABLED;
            DwmSetWindowAttribute(
                hwnd as HWND,
                DWMWA_NCRENDERING_POLICY,
                &ncrp as *const u32 as *const _,
                std::mem::size_of::<u32>() as u32,
            );
            println!("[DesktopEmbedder] DWM: NC rendering policy re-applied (post-SetParent)");
        }

        // 4b. SetParent 后再次 SWP_FRAMECHANGED，确保 NC 区域在新父窗口下也被正确清零
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
            println!("[DesktopEmbedder] Plan A success: WM_NCCALCSIZE interception eliminated NC offset!");

            // ===== 6a. DWM 圆角过扫描补偿（始终启用） =====
            //
            // 问题：24H2 的 DWM 会在合成层面对子窗口应用圆角渲染。
            // DWMWA_WINDOW_CORNER_PREFERENCE 即使在 pre-SetParent 阶段设置成功（hr=0），
            // SetParent 后 DWM 也会重置该属性，导致圆角重新出现。
            //
            // 解决方案：微量过扫描（overscan）
            // 在四边各多扩展 2px，让 DWM 的圆角区域溢出到 WorkerW 的裁剪边界之外。
            // 同时通过 WM_ERASEBKGND 拦截将窗口背景色设为黑色，
            // 防止溢出区域显示为白边（黑色在 DWM 透明合成下不可见）。
            const OVERSCAN: i32 = 2;
            let os_x = target_x - OVERSCAN;
            let os_y = target_y - OVERSCAN;
            let os_w = target_w + OVERSCAN * 2;
            let os_h = target_h + OVERSCAN * 2;
            MoveWindow(hwnd as HWND, os_x, os_y, os_w, os_h, 1);
            println!(
                "[DesktopEmbedder] DWM corner overscan applied: pos=({}, {}), size={}x{} (overscan={}px per edge)",
                os_x, os_y, os_w, os_h, OVERSCAN
            );
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
