//! Win32 显示器几何信息读取的安全包装
//!
//! 把零散的 `GetForegroundWindow` / `MonitorFromWindow` / `MonitorFromPoint`
//! / `GetMonitorInfoW` / `GetWindowRect` 等 unsafe Win32 调用统一封装为
//! Rust 风格的安全 API，向上提供 `Option`、`Result` 友好的签名，
//! 让业务模块（active_monitor、fullscreen_detector 等）无需直接接触 unsafe。
//!
//! ## 设计原则
//!
//! - **零业务语义**：本模块不做"是否桌面"、"是否全屏"等业务判定，
//!   只暴露原子几何能力，业务语义交给上层模块处理
//! - **device_name 一致性**：所有显示器查询统一使用 `MONITORINFOEXW`，
//!   `device_name` 字段（形如 `\\.\DISPLAY1`）与 Tauri 前端
//!   `availableMonitors().name` 同源，可直接与 DB `monitor_configs.monitor_id` 比较
//! - **跨平台占位**：非 Windows 平台所有函数返回 `None` 占位，
//!   保证业务代码在任意平台都能通过编译

#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::HWND;

/// 显示器的完整几何 + 标识信息
#[derive(Debug, Clone)]
pub struct MonitorRect {
    /// 设备名称，形如 `\\.\DISPLAY1`，与 DB `monitor_configs.monitor_id` 同源
    pub device_name: String,
    /// 显示器物理矩形 - 左
    pub left: i32,
    /// 显示器物理矩形 - 上
    pub top: i32,
    /// 显示器物理矩形 - 右
    pub right: i32,
    /// 显示器物理矩形 - 下
    pub bottom: i32,
    /// 工作区矩形（`rcWork`，已排除任务栏/停靠工具栏）- 左
    ///
    /// 与 `left`/`top`/`right`/`bottom`（`rcMonitor`，显示器全尺寸）的区别：
    /// 全屏判定必须用 rcMonitor，而"把窗口摆在屏幕角落"这类定位应当用 rcWork，
    /// 否则窗口会被任务栏遮住。两者在无任务栏的副屏上通常相等。
    pub work_left: i32,
    /// 工作区矩形 - 上
    pub work_top: i32,
    /// 工作区矩形 - 右
    pub work_right: i32,
    /// 工作区矩形 - 下
    pub work_bottom: i32,
}

impl MonitorRect {
    /// 显示器宽度（物理像素）
    #[inline]
    #[allow(dead_code)]
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    /// 显示器高度（物理像素）
    #[inline]
    #[allow(dead_code)]
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    /// 工作区宽度（物理像素，已排除任务栏）
    #[inline]
    pub fn work_width(&self) -> i32 {
        self.work_right - self.work_left
    }

    /// 工作区高度（物理像素，已排除任务栏）
    #[inline]
    pub fn work_height(&self) -> i32 {
        self.work_bottom - self.work_top
    }
}

/// 简单 Rect，仅承载 left/top/right/bottom，用于窗口矩形等场景
#[derive(Debug, Clone, Copy)]
pub struct WinRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl WinRect {
    /// 判定本矩形是否完全覆盖目标显示器矩形（用于全屏判定）
    #[inline]
    pub fn covers_monitor(&self, mon: &MonitorRect) -> bool {
        self.left <= mon.left
            && self.top <= mon.top
            && self.right >= mon.right
            && self.bottom >= mon.bottom
    }
}

/// 枚举显示器得到的完整信息：物理几何 + 该屏有效 DPI。
///
/// `rect` 里的坐标恒为物理像素；`dpi` 是 `MDT_EFFECTIVE_DPI`，
/// 用于把物理像素换算成 Tauri WebviewWindowBuilder 需要的逻辑像素。
/// 非 Windows 平台不会产出该结构体，但定义保持跨平台可见，便于上层类型统一。
#[derive(Debug, Clone)]
pub struct MonitorInfo {
    pub rect: MonitorRect,
    pub dpi: u32,
}

impl MonitorInfo {
    /// DPI 缩放系数（96 DPI = 1.0）。查询失败时回落到的 96 也自然得到 1.0。
    #[inline]
    pub fn scale_factor(&self) -> f64 {
        self.dpi as f64 / 96.0
    }
}

// ===================== Windows 实现 =====================

#[cfg(target_os = "windows")]
mod imp {
    use super::{MonitorInfo, MonitorRect, WinRect, HWND};
    use windows_sys::Win32::Foundation::{BOOL, LPARAM, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, HDC, HMONITOR,
        MONITORINFOEXW, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL, MONITOR_DEFAULTTOPRIMARY,
    };
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetCursorPos, GetDesktopWindow, GetForegroundWindow, GetWindowRect,
        GetWindowTextW,
    };

    /// 获取前台窗口 HWND，无前台窗口或为桌面窗口时返回 `None`
    ///
    /// 注意：
    /// - 仅过滤掉"返回桌面窗口本身"这一种特殊情况
    /// - 不过滤 Shell_TrayWnd / Progman / WorkerW 等系统窗口类，
    ///   是否把它们视为合法前台窗口由上层业务决定
    pub fn foreground_window() -> Option<HWND> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return None;
            }
            if hwnd == GetDesktopWindow() {
                return None;
            }
            Some(hwnd)
        }
    }

    /// 读取窗口的类名（仅前 256 字符），失败返回 `None`
    ///
    /// 上层可用此判断是否为系统桌面/任务栏窗口（Shell_TrayWnd / Progman / WorkerW）
    pub fn window_class_name(hwnd: HWND) -> Option<String> {
        unsafe {
            let mut buf = [0u16; 256];
            let len = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            if len <= 0 {
                return None;
            }
            Some(String::from_utf16_lossy(&buf[..len as usize]))
        }
    }

    /// 读取窗口标题（仅前 256 字符），失败或无标题返回 `None`
    ///
    /// 上层可用此判断是否为本应用自己创建的无边框窗口（壁纸 / Toast 等
    /// 不依赖窗口类名、而是用显式设置的 title 区分的场景）。
    fn window_title(hwnd: HWND) -> Option<String> {
        unsafe {
            let mut buf = [0u16; 256];
            let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            if len <= 0 {
                return None;
            }
            Some(String::from_utf16_lossy(&buf[..len as usize]))
        }
    }

    /// 判定 HWND 是否属于本应用自己创建的壁纸窗口（Native 渲染表面 / WebView 壁纸窗口）
    ///
    /// 壁纸窗口稳态下嵌入在 WorkerW 之下、永远不会成为前台窗口（见 `active_monitor`
    /// 的设计说明）；但跨 backend 切换销毁旧窗口时，会经历「`SetParent(null)` 解除
    /// 嵌入 → `DestroyWindow`」之间的瞬时裸露期——解除嵌入后它短暂变回顶层窗口，
    /// Native 窗口在嵌入时还被清掉过 `WS_EX_NOACTIVATE`，这个窗口期内有极小概率被
    /// 系统误判为前台应用。按窗口类名（Native 用 `MiniWallpaperNativeRenderer`）
    /// 和标题（WebView 壁纸窗口固定标题为 `Wallpaper`）排除，避免
    /// `fullscreen_detector` 在切换瞬间误报全屏。
    pub fn is_own_wallpaper_window(hwnd: HWND) -> bool {
        if window_class_name(hwnd).as_deref() == Some("MiniWallpaperNativeRenderer") {
            return true;
        }
        window_title(hwnd).as_deref() == Some("Wallpaper")
    }

    /// 获取窗口矩形（屏幕坐标）
    pub fn window_rect(hwnd: HWND) -> Option<WinRect> {
        unsafe {
            let mut rect: RECT = std::mem::zeroed();
            if GetWindowRect(hwnd, &mut rect) == 0 {
                return None;
            }
            Some(WinRect {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            })
        }
    }

    /// 获取当前鼠标位置（屏幕坐标）
    pub fn cursor_pos() -> Option<(i32, i32)> {
        unsafe {
            let mut pt: POINT = std::mem::zeroed();
            if GetCursorPos(&mut pt) == 0 {
                return None;
            }
            Some((pt.x, pt.y))
        }
    }

    /// 给定 HWND，获取所在显示器的完整信息（含 device_name）
    ///
    /// `MONITOR_DEFAULTTONEAREST`：始终返回最近的显示器（不会返回 NULL）。
    pub fn monitor_info_from_window(hwnd: HWND) -> Option<MonitorRect> {
        unsafe {
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            read_monitor_info(monitor)
        }
    }

    /// 给定屏幕坐标点，获取所在显示器的完整信息
    ///
    /// `MONITOR_DEFAULTTONULL`：点不在任何显示器内时返回 `None`，
    /// 由上层决定是否再 fallback 到主显示器。
    pub fn monitor_info_from_point(x: i32, y: i32) -> Option<MonitorRect> {
        unsafe {
            let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONULL);
            read_monitor_info(monitor)
        }
    }

    /// 获取主显示器
    ///
    /// 实现：以 (0, 0) 为参考点 + `MONITOR_DEFAULTTOPRIMARY`，
    /// Windows 保证主显示器原点位于 (0, 0)，因此该方式总能命中主屏。
    pub fn primary_monitor_info() -> Option<MonitorRect> {
        unsafe {
            let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            read_monitor_info(monitor)
        }
    }

    /// 内部辅助：从 HMONITOR 句柄读取 MONITORINFOEXW 并构建 MonitorRect
    unsafe fn read_monitor_info(monitor: HMONITOR) -> Option<MonitorRect> {
        if monitor.is_null() {
            return None;
        }

        // 使用 MONITORINFOEXW 而非 MONITORINFO，多出的 szDevice 字段是 \\.\DISPLAYn
        let mut info: MONITORINFOEXW = std::mem::zeroed();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;

        // GetMonitorInfoW 接受 *mut MONITORINFO，EXW 指针强转传入即可
        let info_ptr = &mut info as *mut MONITORINFOEXW as *mut _;
        if GetMonitorInfoW(monitor, info_ptr) == 0 {
            return None;
        }

        // szDevice 是 [u16; 32] 的 C 字符串，截取到第一个 \0
        let len = info
            .szDevice
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(info.szDevice.len());
        let device_name = String::from_utf16_lossy(&info.szDevice[..len]);

        let rect = info.monitorInfo.rcMonitor;
        let work = info.monitorInfo.rcWork;
        Some(MonitorRect {
            device_name,
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
            work_left: work.left,
            work_top: work.top,
            work_right: work.right,
            work_bottom: work.bottom,
        })
    }

    /// 判定 HWND 是否属于"系统桌面/任务栏"类窗口（Shell_TrayWnd / Progman / WorkerW）
    ///
    /// 业务侧（如 fullscreen_detector）可调用此方法过滤掉桌面/任务栏的误判。
    pub fn is_shell_window(hwnd: HWND) -> bool {
        match window_class_name(hwnd).as_deref() {
            Some("Shell_TrayWnd") | Some("Progman") | Some("WorkerW") => true,
            _ => false,
        }
    }

    /// 枚举当前所有物理显示器，返回各自几何 + 有效 DPI。
    ///
    /// 用 EnumDisplayMonitors 而不是按坐标逐个 MonitorFromPoint，才能完整覆盖
    /// 负坐标布局、多屏不同 DPI、非矩形排列的虚拟桌面。device_name 仍由
    /// read_monitor_info 从 MONITORINFOEXW 取得，与 DB monitor_id 同源。
    pub fn enumerate_monitors() -> Vec<MonitorInfo> {
        let mut monitors: Vec<MonitorInfo> = Vec::new();
        unsafe {
            EnumDisplayMonitors(
                std::ptr::null_mut(),
                std::ptr::null(),
                Some(collect_monitor),
                &mut monitors as *mut Vec<MonitorInfo> as LPARAM,
            );
        }
        monitors
    }

    /// 回调只负责把 HMONITOR 解析成几何 + DPI 追加到 Vec，不做业务判定。
    /// 单屏查询失败时跳过该屏并返回 TRUE 继续枚举，避免一块坏屏拖垮整组。
    unsafe extern "system" fn collect_monitor(
        monitor: HMONITOR,
        _hdc: HDC,
        _clip: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let out = &mut *(data as *mut Vec<MonitorInfo>);
        let Some(rect) = (unsafe { read_monitor_info(monitor) }) else {
            return 1;
        };

        let mut dpi_x = 0u32;
        let mut dpi_y = 0u32;
        // GetDpiForMonitor 返回 HRESULT，S_OK 为 0。失败或非法的 0 值回落到
        // 96 DPI（scale_factor=1.0），保证 shcore 异常时该屏不会从枚举里消失。
        let dpi = if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) == 0
            && dpi_x > 0
        {
            dpi_x
        } else {
            96
        };
        out.push(MonitorInfo { rect, dpi });
        1
    }
}

#[cfg(target_os = "windows")]
pub use imp::{
    cursor_pos, enumerate_monitors, foreground_window, is_own_wallpaper_window, is_shell_window,
    monitor_info_from_point, monitor_info_from_window, primary_monitor_info, window_rect,
};

// ===================== 非 Windows 平台占位 =====================

#[cfg(not(target_os = "windows"))]
mod imp_stub {
    use super::{MonitorInfo, MonitorRect, WinRect};

    /// 非 Windows 平台占位类型，仅用于让签名通过编译
    pub type HWND = *mut std::ffi::c_void;

    /// 非 Windows 平台没有 Win32 显示器枚举，返回空集由上层按无屏处理。
    pub fn enumerate_monitors() -> Vec<MonitorInfo> {
        Vec::new()
    }

    pub fn foreground_window() -> Option<HWND> {
        None
    }

    pub fn window_class_name(_hwnd: HWND) -> Option<String> {
        None
    }

    pub fn window_rect(_hwnd: HWND) -> Option<WinRect> {
        None
    }

    pub fn cursor_pos() -> Option<(i32, i32)> {
        None
    }

    pub fn monitor_info_from_window(_hwnd: HWND) -> Option<MonitorRect> {
        None
    }

    pub fn monitor_info_from_point(_x: i32, _y: i32) -> Option<MonitorRect> {
        None
    }

    pub fn primary_monitor_info() -> Option<MonitorRect> {
        None
    }

    pub fn is_shell_window(_hwnd: HWND) -> bool {
        false
    }

    pub fn is_own_wallpaper_window(_hwnd: HWND) -> bool {
        false
    }
}

#[cfg(not(target_os = "windows"))]
pub use imp_stub::{
    cursor_pos, enumerate_monitors, foreground_window, is_own_wallpaper_window, is_shell_window,
    monitor_info_from_point, monitor_info_from_window, primary_monitor_info, window_class_name,
    window_rect, HWND,
};

// ===================== 高阶编排 API =====================

/// 探测"用户当前正在使用的显示器"
///
/// 用于：
/// - **Toast 弹窗定位**：决定提示窗口弹在哪块屏的右下角
/// - **Independent 模式动作派发**：决定全局快捷键/托盘动作作用于哪个 `monitor_config`
/// - **托盘菜单 active 标记**：用于在菜单中标注"当前活跃屏"
///
/// ## 探测策略（两级 fallback）
///
/// 1. `GetCursorPos` 鼠标所在显示器
/// 2. 鼠标位置不可用时回落到主显示器
///
/// ## 为什么不用前台窗口
///
/// 壁纸窗口是嵌入桌面的子窗口（`always_on_bottom` + SetParent 到 WorkerW），
/// 永远不会成为前台窗口；用户在副屏悬停/操作壁纸时，前台仍是资源管理器、
/// 任务栏或 Progman/WorkerW（覆盖整个虚拟桌面，锚点在主屏）。用
/// `GetForegroundWindow` 解析会恒命中主屏，导致副屏壁纸无法被操作。
/// 鼠标位置是壁纸交互中唯一始终跟随用户意图的信号，故只用它做判定。
///
/// ## device_name 一致性
///
/// 返回值的 `device_name` 形如 `\\.\DISPLAY1`，
/// 与 Tauri `availableMonitors().name` 同源，
/// 可直接与 DB `monitor_configs.monitor_id` 进行字符串相等比较。
///
/// ## 设计说明
///
/// - 返回 `None` 仅在 `primary_monitor_info()` 也失败的极端情况下出现，
///   调用方通常应当再次 fallback 到"广播所有 active 显示器"或忽略本次操作。
/// - 非 Windows 平台始终返回 `None`（底层原子能力均为占位）。
pub fn active_monitor() -> Option<MonitorRect> {
    // ---- 第 1 级：鼠标所在显示器 ----
    if let Some((x, y)) = cursor_pos() {
        if let Some(info) = monitor_info_from_point(x, y) {
            return Some(info);
        }
    }

    // ---- 第 2 级：主显示器兜底 ----
    primary_monitor_info()
}
