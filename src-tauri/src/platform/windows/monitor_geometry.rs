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
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    /// 显示器高度（物理像素）
    #[inline]
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

// ===================== Windows 实现 =====================

#[cfg(target_os = "windows")]
mod imp {
    use super::{MonitorRect, WinRect, HWND};
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, HMONITOR, MONITORINFOEXW,
        MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL, MONITOR_DEFAULTTOPRIMARY,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetCursorPos, GetDesktopWindow, GetForegroundWindow, GetWindowRect,
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
}

#[cfg(target_os = "windows")]
pub use imp::{
    cursor_pos, foreground_window, is_shell_window, monitor_info_from_point,
    monitor_info_from_window, primary_monitor_info, window_class_name, window_rect,
};

// ===================== 非 Windows 平台占位 =====================

#[cfg(not(target_os = "windows"))]
mod imp_stub {
    use super::{MonitorRect, WinRect};

    /// 非 Windows 平台占位类型，仅用于让签名通过编译
    pub type HWND = *mut std::ffi::c_void;

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
}

#[cfg(not(target_os = "windows"))]
pub use imp_stub::{
    cursor_pos, foreground_window, is_shell_window, monitor_info_from_point,
    monitor_info_from_window, primary_monitor_info, window_class_name, window_rect, HWND,
};

// ===================== 高阶编排 API =====================

/// 探测"用户当前正在使用的显示器"
///
/// 用于：
/// - **Toast 弹窗定位**：决定提示窗口弹在哪块屏的右下角
/// - **Independent 模式动作派发**：决定全局快捷键/托盘动作作用于哪个 `monitor_config`
/// - **托盘菜单 active 标记**：用于在菜单中标注"当前活跃屏"
///
/// ## 探测策略（三级 fallback）
///
/// 1. 优先使用 `GetForegroundWindow` 所在显示器（用户视觉焦点最准确的来源）
/// 2. 若前台窗口不可用，退化为 `GetCursorPos` 鼠标所在显示器
/// 3. 若鼠标位置也不可用，最终回落到主显示器
///
/// ## device_name 一致性
///
/// 返回值的 `device_name` 形如 `\\.\DISPLAY1`，
/// 与 Tauri `availableMonitors().name` 同源，
/// 可直接与 DB `monitor_configs.monitor_id` 进行字符串相等比较。
///
/// ## 设计说明
///
/// - 第 1 级**不排除** Shell_TrayWnd / Progman / WorkerW 等系统窗口类，
///   因为用户聚焦在桌面/任务栏时，提示仍然应该弹在该屏幕。
/// - 返回 `None` 仅在所有 fallback 路径都失败的极端情况下出现，
///   调用方通常应当再次 fallback 到"广播所有 active 显示器"或忽略本次操作。
/// - 非 Windows 平台始终返回 `None`（底层原子能力均为占位）。
pub fn active_monitor() -> Option<MonitorRect> {
    // ---- 第 1 级：前台窗口所在显示器（最贴近用户视觉焦点）----
    if let Some(hwnd) = foreground_window() {
        if let Some(info) = monitor_info_from_window(hwnd) {
            return Some(info);
        }
    }

    // ---- 第 2 级：鼠标所在显示器 ----
    if let Some((x, y)) = cursor_pos() {
        if let Some(info) = monitor_info_from_point(x, y) {
            return Some(info);
        }
    }

    // ---- 第 3 级：主显示器兜底 ----
    primary_monitor_info()
}