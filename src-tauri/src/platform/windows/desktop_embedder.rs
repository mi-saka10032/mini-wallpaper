//! Win32 桌面壁纸嵌入模块
//!
//! 核心原理：
//! 1. FindWindow("Progman") 找到桌面窗口
//! 2. SendMessageTimeout(Progman, 0x052C) 触发 explorer 创建 WorkerW
//! 3. 根据 Windows 版本选择不同的嵌入策略（策略模式）：
//!    - 24H2+：WndProc 子类化 + 样式清理 + NC 补偿
//!    - 旧版本：经典 EnumWindows + 简单 SetParent
//! 4. SetParent(tauri_hwnd, workerw) 将壁纸窗口嵌入桌面层级
//!
//! 架构设计：
//! - `EmbedStrategy` trait 定义嵌入策略接口
//! - `ModernStrategy`（24H2+）和 `LegacyStrategy`（旧版本）分别实现
//! - `select_strategy()` 根据版本自动选择策略
//! - 日志使用 `log` crate，错误处理使用 `anyhow`

#[cfg(target_os = "windows")]
use anyhow::{bail, Context, Result};
#[cfg(target_os = "windows")]
use log::{debug, error, info, warn};
#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicIsize, Ordering};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, SendMessageTimeoutW, SetParent, SMTO_NORMAL,
};

// ============================================================
// 嵌入策略 trait
// ============================================================

/// 桌面嵌入策略接口
///
/// 不同 Windows 版本的嵌入行为差异通过策略模式封装，
/// 调用方无需关心版本细节。
#[cfg(target_os = "windows")]
trait EmbedStrategy {
    /// 策略名称（用于日志）
    fn name(&self) -> &'static str;

    /// 查找目标 WorkerW 窗口
    fn find_workerw(&self, progman: HWND) -> Result<HWND>;

    /// 执行嵌入操作（SetParent + 平台特定的样式修复）
    fn embed(&self, hwnd: HWND, workerw: HWND, rect: MonitorRect) -> Result<()>;
}

/// 显示器矩形区域（虚拟桌面坐标 + 物理分辨率）
#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy)]
struct MonitorRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

// ============================================================
// WndProc 子类化基础设施（仅 24H2+ 使用）
// ============================================================

/// 子类化槽位：保存原始 WndProc 和对应 HWND（支持最多 8 个显示器）
#[cfg(target_os = "windows")]
static ORIGINAL_WNDPROCS: [AtomicIsize; 8] = [
    AtomicIsize::new(0), AtomicIsize::new(0), AtomicIsize::new(0), AtomicIsize::new(0),
    AtomicIsize::new(0), AtomicIsize::new(0), AtomicIsize::new(0), AtomicIsize::new(0),
];

#[cfg(target_os = "windows")]
static SUBCLASSED_HWNDS: [AtomicIsize; 8] = [
    AtomicIsize::new(0), AtomicIsize::new(0), AtomicIsize::new(0), AtomicIsize::new(0),
    AtomicIsize::new(0), AtomicIsize::new(0), AtomicIsize::new(0), AtomicIsize::new(0),
];

/// 根据 HWND 查找已注册的原始 WndProc
#[cfg(target_os = "windows")]
fn find_original_wndproc(hwnd: HWND) -> Option<isize> {
    let hwnd_val = hwnd as isize;
    SUBCLASSED_HWNDS.iter().zip(ORIGINAL_WNDPROCS.iter()).find_map(|(h, p)| {
        if h.load(Ordering::SeqCst) == hwnd_val {
            let proc = p.load(Ordering::SeqCst);
            (proc != 0).then_some(proc)
        } else {
            None
        }
    })
}

/// 注册子类化信息到空闲槽位
#[cfg(target_os = "windows")]
fn register_subclass(hwnd: HWND, original_proc: isize) {
    let hwnd_val = hwnd as isize;
    for (i, (h, p)) in SUBCLASSED_HWNDS.iter().zip(ORIGINAL_WNDPROCS.iter()).enumerate() {
        let existing = h.load(Ordering::SeqCst);
        if existing == 0 || existing == hwnd_val {
            h.store(hwnd_val, Ordering::SeqCst);
            p.store(original_proc, Ordering::SeqCst);
            debug!(
                "子类化已注册: slot={}, hwnd=0x{:X}, orig_proc=0x{:X}",
                i, hwnd_val, original_proc
            );
            return;
        }
    }
    warn!("无空闲子类化槽位: hwnd=0x{:X}", hwnd_val);
}

/// 子类化窗口过程：拦截 WM_NCCALCSIZE，强制 NC 区域为 0
#[cfg(target_os = "windows")]
unsafe extern "system" fn subclass_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{CallWindowProcW, WM_NCCALCSIZE};

    if msg == WM_NCCALCSIZE {
        return 0;
    }

    if let Some(original_proc) = find_original_wndproc(hwnd) {
        type WndProcFn = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;
        let fn_ptr: WndProcFn = std::mem::transmute(original_proc as usize);
        CallWindowProcW(Some(fn_ptr), hwnd, msg, wparam, lparam)
    } else {
        windows_sys::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

// ============================================================
// 版本检测
// ============================================================

/// 检测当前 Windows 版本是否为 24H2+（Build >= 26100）
///
/// 使用 ntdll.dll 的 RtlGetVersion 获取真实 Build Number，
/// 不受 manifest 兼容性限制影响。
#[cfg(target_os = "windows")]
fn is_win11_24h2_or_later() -> bool {
    use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;

    unsafe {
        let mut osvi: OSVERSIONINFOW = std::mem::zeroed();
        osvi.dwOSVersionInfoSize = std::mem::size_of::<OSVERSIONINFOW>() as u32;

        type RtlGetVersionFn = unsafe extern "system" fn(*mut OSVERSIONINFOW) -> i32;

        let ntdll = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(
            encode_wide("ntdll.dll\0").as_ptr(),
        );
        if ntdll == std::ptr::null_mut() {
            warn!("无法获取 ntdll.dll 句柄，降级到经典路径");
            return false;
        }

        let proc = windows_sys::Win32::System::LibraryLoader::GetProcAddress(
            ntdll,
            b"RtlGetVersion\0".as_ptr(),
        );
        if proc.is_none() {
            warn!("无法获取 RtlGetVersion，降级到经典路径");
            return false;
        }

        let rtl_get_version: RtlGetVersionFn = std::mem::transmute(proc);
        let status = rtl_get_version(&mut osvi as *mut OSVERSIONINFOW);

        if status == 0 {
            let build = osvi.dwBuildNumber;
            let is_24h2 = build >= 26100;
            info!(
                "Windows 版本: {}.{}.{} → {}",
                osvi.dwMajorVersion,
                osvi.dwMinorVersion,
                build,
                if is_24h2 { "24H2+ (Modern)" } else { "Legacy (Classic)" }
            );
            is_24h2
        } else {
            warn!("RtlGetVersion 失败 (status={})，降级到经典路径", status);
            false
        }
    }
}

// ============================================================
// ModernStrategy — 24H2+ 嵌入策略
// ============================================================

/// 24H2+ 嵌入策略
///
/// Windows 11 24H2 改变了桌面窗口层级（WorkerW 变为 Progman 子窗口），
/// 且 SetParent 后 DWM 会通过 WM_NCCALCSIZE 注入隐藏 NC 边框。
///
/// 处理流程：
/// 1. FindWindowExW(Progman, "WorkerW") 直接查找子窗口
/// 2. 子类化 WndProc 拦截 WM_NCCALCSIZE
/// 3. 清除所有边框样式
/// 4. SetParent 嵌入
/// 5. 验证 NC 偏移，必要时 MoveWindow 补偿
#[cfg(target_os = "windows")]
struct ModernStrategy;

#[cfg(target_os = "windows")]
impl EmbedStrategy for ModernStrategy {
    fn name(&self) -> &'static str {
        "Modern (24H2+)"
    }

    fn find_workerw(&self, progman: HWND) -> Result<HWND> {
        unsafe {
            let w = FindWindowExW(
                progman,
                std::ptr::null_mut(),
                encode_wide("WorkerW\0").as_ptr(),
                std::ptr::null(),
            );
            info!("24H2+ WorkerW 查找: FindWindowExW → {:?}", w);

            if w == std::ptr::null_mut() {
                warn!("24H2+ 路径失败，fallback 到经典 EnumWindows");
                return find_workerw_classic();
            }
            Ok(w)
        }
    }

    fn embed(&self, hwnd: HWND, workerw: HWND, rect: MonitorRect) -> Result<()> {
        use windows_sys::Win32::{
            Graphics::Gdi::ClientToScreen,
            UI::WindowsAndMessaging::{
                GetClientRect, GetWindowLongPtrW, GetWindowRect, MoveWindow,
                SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos,
                GWL_EXSTYLE, GWL_STYLE, GWL_WNDPROC, LWA_ALPHA,
                SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
                WS_BORDER, WS_CAPTION, WS_CHILD, WS_DLGFRAME,
                WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME, WS_EX_LAYERED,
                WS_EX_STATICEDGE, WS_EX_TRANSPARENT, WS_EX_WINDOWEDGE,
                WS_THICKFRAME, WS_VISIBLE,
            },
        };

        unsafe {
            // 1. 子类化 WndProc：拦截 WM_NCCALCSIZE
            let original_proc = GetWindowLongPtrW(hwnd, GWL_WNDPROC);
            if original_proc != 0 {
                register_subclass(hwnd, original_proc);
                let new_proc = subclass_wndproc as isize;
                SetWindowLongPtrW(hwnd, GWL_WNDPROC, new_proc);
                debug!("WndProc 已替换: orig=0x{:X}, new=0x{:X}", original_proc, new_proc);
            } else {
                warn!("GetWindowLongPtrW(GWL_WNDPROC) 返回 0，跳过子类化");
            }

            // 2. 清除窗口样式中的所有边框位
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
            let clean_style = (style
                & !(WS_CAPTION as isize)
                & !(WS_THICKFRAME as isize)
                & !(WS_BORDER as isize)
                & !(WS_DLGFRAME as isize))
                | WS_CHILD as isize
                | WS_VISIBLE as isize;
            SetWindowLongPtrW(hwnd, GWL_STYLE, clean_style);

            // 3. 清除扩展样式边框 + 设置 Layered + Transparent
            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let clean_ex = (ex_style
                & !(WS_EX_CLIENTEDGE as isize)
                & !(WS_EX_STATICEDGE as isize)
                & !(WS_EX_WINDOWEDGE as isize)
                & !(WS_EX_DLGMODALFRAME as isize))
                | WS_EX_LAYERED as isize
                | WS_EX_TRANSPARENT as isize;
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, clean_ex);

            // 4. Layered 窗口完全不透明
            SetLayeredWindowAttributes(hwnd, 0, 0xFF, LWA_ALPHA);

            // 5. SWP_FRAMECHANGED 触发 WM_NCCALCSIZE（被我们的 WndProc 拦截返回 0）
            SetWindowPos(
                hwnd, std::ptr::null_mut(), 0, 0, 0, 0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE,
            );

            debug!("样式已清理，SWP_FRAMECHANGED 已发送");

            // 6. SetParent 嵌入
            let prev_parent = SetParent(hwnd, workerw);
            if prev_parent == std::ptr::null_mut() {
                bail!("SetParent 失败");
            }

            // 7. 嵌入后再次 FRAMECHANGED
            SetWindowPos(
                hwnd, std::ptr::null_mut(), 0, 0, 0, 0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE,
            );

            // 8. 定位到目标显示器
            MoveWindow(hwnd, rect.x, rect.y, rect.width, rect.height, 1);

            // 9. 验证 NC 偏移
            let nc = measure_nc_offset(hwnd);
            info!(
                "NC 偏移验证: L={} T={} R={} B={}, client={}x{}, target={}x{}",
                nc.left, nc.top, nc.right, nc.bottom,
                nc.client_w, nc.client_h, rect.width, rect.height
            );

            if nc.has_offset() {
                warn!("NC 偏移仍存在，执行 MoveWindow 补偿");
                let comp_x = rect.x - nc.left;
                let comp_y = rect.y - nc.top;
                let comp_w = rect.width + nc.left + nc.right;
                let comp_h = rect.height + nc.top + nc.bottom;
                MoveWindow(hwnd, comp_x, comp_y, comp_w, comp_h, 1);
                info!("NC 补偿完成: pos=({}, {}), size={}x{}", comp_x, comp_y, comp_w, comp_h);
            } else {
                info!("WM_NCCALCSIZE 拦截成功，NC 偏移已消除");
            }

            Ok(())
        }
    }
}

// ============================================================
// LegacyStrategy — 旧版本嵌入策略
// ============================================================

/// 旧版本嵌入策略（Win7 ~ Win11 23H2）
///
/// 经典方案：EnumWindows 查找 SHELLDLL_DefView 所在 WorkerW 的前一个兄弟，
/// 简单 SetParent + MoveWindow，无需额外修复。
#[cfg(target_os = "windows")]
struct LegacyStrategy;

#[cfg(target_os = "windows")]
impl EmbedStrategy for LegacyStrategy {
    fn name(&self) -> &'static str {
        "Legacy (Classic)"
    }

    fn find_workerw(&self, _progman: HWND) -> Result<HWND> {
        find_workerw_classic()
    }

    fn embed(&self, hwnd: HWND, workerw: HWND, rect: MonitorRect) -> Result<()> {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, MoveWindow, SetWindowLongPtrW,
            GWL_EXSTYLE, WS_EX_TRANSPARENT,
        };

        unsafe {
            // 设置鼠标穿透
            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_TRANSPARENT as isize);

            let prev_parent = SetParent(hwnd, workerw);
            if prev_parent == std::ptr::null_mut() {
                bail!("SetParent 失败");
            }

            MoveWindow(hwnd, rect.x, rect.y, rect.width, rect.height, 1);
            info!("经典嵌入完成: SetParent + MoveWindow");
            Ok(())
        }
    }
}

// ============================================================
// NC 偏移测量
// ============================================================

/// NC 偏移测量结果
#[cfg(target_os = "windows")]
struct NcOffset {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    client_w: i32,
    client_h: i32,
}

#[cfg(target_os = "windows")]
impl NcOffset {
    fn has_offset(&self) -> bool {
        self.left != 0 || self.top != 0 || self.right != 0 || self.bottom != 0
    }
}

/// 测量窗口的 NC（非客户区）偏移
#[cfg(target_os = "windows")]
unsafe fn measure_nc_offset(hwnd: HWND) -> NcOffset {
    use windows_sys::Win32::{
        Graphics::Gdi::ClientToScreen,
        UI::WindowsAndMessaging::{GetClientRect, GetWindowRect},
    };

    let mut win_rect: RECT = std::mem::zeroed();
    GetWindowRect(hwnd, &mut win_rect);

    let mut client_rect: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut client_rect);

    let mut client_origin = POINT { x: 0, y: 0 };
    ClientToScreen(hwnd, &mut client_origin);

    NcOffset {
        left: client_origin.x - win_rect.left,
        top: client_origin.y - win_rect.top,
        right: win_rect.right - (client_origin.x + client_rect.right),
        bottom: win_rect.bottom - (client_origin.y + client_rect.bottom),
        client_w: client_rect.right - client_rect.left,
        client_h: client_rect.bottom - client_rect.top,
    }
}

// ============================================================
// 经典 WorkerW 查找（EnumWindows 方案）
// ============================================================

/// 通过 EnumWindows 查找桌面 WorkerW（经典方案）
///
/// 找到包含 SHELLDLL_DefView 的 WorkerW，取其在 Z-order 上的前一个 WorkerW 兄弟
#[cfg(target_os = "windows")]
fn find_workerw_classic() -> Result<HWND> {
    static FOUND_WORKERW: AtomicIsize = AtomicIsize::new(0);
    FOUND_WORKERW.store(0, Ordering::SeqCst);

    unsafe extern "system" fn enum_callback(hwnd: HWND, _lparam: LPARAM) -> BOOL {
        let shell_view = FindWindowExW(
            hwnd,
            std::ptr::null_mut(),
            encode_wide("SHELLDLL_DefView\0").as_ptr(),
            std::ptr::null(),
        );

        if shell_view != std::ptr::null_mut() {
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

    unsafe { EnumWindows(Some(enum_callback), 0) };

    let result = FOUND_WORKERW.load(Ordering::SeqCst);
    if result == 0 {
        bail!("经典方案未找到 WorkerW 窗口");
    }
    Ok(result as HWND)
}

// ============================================================
// 策略选择 + 公共 API
// ============================================================

/// 根据 Windows 版本选择嵌入策略
#[cfg(target_os = "windows")]
fn select_strategy() -> Box<dyn EmbedStrategy> {
    if is_win11_24h2_or_later() {
        Box::new(ModernStrategy)
    } else {
        Box::new(LegacyStrategy)
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
) -> Result<()> {
    let rect = MonitorRect {
        x: monitor_x,
        y: monitor_y,
        width: monitor_width,
        height: monitor_height,
    };

    info!("开始嵌入: HWND=0x{:X}, 显示器区域={:?}", hwnd, rect);

    unsafe {
        // 1. 找到 Progman 窗口
        let progman = FindWindowW(encode_wide("Progman\0").as_ptr(), std::ptr::null());
        if progman == std::ptr::null_mut() {
            bail!("未找到 Progman 窗口");
        }

        // 2. 发送 0x052C 消息触发 WorkerW 创建
        let mut _result: usize = 0;
        SendMessageTimeoutW(progman, 0x052C, 0, 0, SMTO_NORMAL, 1000, &mut _result as *mut usize);

        // 3. 选择嵌入策略
        let strategy = select_strategy();
        info!("使用嵌入策略: {}", strategy.name());

        // 4. 查找 WorkerW
        let workerw = strategy
            .find_workerw(progman)
            .context("WorkerW 查找失败")?;

        // 5. 执行嵌入
        strategy
            .embed(hwnd as HWND, workerw, rect)
            .context("嵌入操作失败")?;

        // 注意：DWM 圆角裁剪（~1px）已确认无法通过任何窗口级 API 消除，
        // Overscan 方案虽能消除圆角但会导致多屏交接处溢出更明显（色差壁纸下尤为突出）。
        // 权衡后选择接受 1px 圆角溢出——面积更小、更可控。

        info!(
            "嵌入完成: HWND=0x{:X} → WorkerW={:?}, pos=({},{}), size={}x{}",
            hwnd, workerw, rect.x, rect.y, rect.width, rect.height
        );

        Ok(())
    }
}

/// 从桌面层级中移除嵌入的窗口
#[cfg(target_os = "windows")]
pub fn unembed_from_desktop(hwnd: isize) {
    unsafe {
        SetParent(hwnd as HWND, std::ptr::null_mut());
        info!("已解除嵌入: HWND=0x{:X}", hwnd);
    }
}

/// 辅助函数：将 &str 编码为以 null 结尾的 UTF-16 Vec
#[cfg(target_os = "windows")]
fn encode_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

// ============================================================
// 非 Windows 平台的空实现
// ============================================================

#[cfg(not(target_os = "windows"))]
pub fn embed_in_desktop(
    _hwnd: isize,
    _monitor_x: i32,
    _monitor_y: i32,
    _monitor_width: i32,
    _monitor_height: i32,
) -> anyhow::Result<()> {
    log::info!("embed_in_desktop 在当前平台为空操作");
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn unembed_from_desktop(_hwnd: isize) {
    log::info!("unembed_from_desktop 在当前平台为空操作");
}
