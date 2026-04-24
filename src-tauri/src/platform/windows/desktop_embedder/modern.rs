//! 24H2+ 嵌入策略（ModernStrategy）
//!
//! Windows 11 24H2 改变了桌面窗口层级（WorkerW 变为 Progman 子窗口），
//! 且 SetParent 后 DWM 会通过 WM_NCCALCSIZE 注入隐藏 NC 边框。
//!
//! 处理流程：
//! 1. FindWindowExW(Progman, "WorkerW") 直接查找子窗口
//! 2. 子类化 WndProc 拦截 WM_NCCALCSIZE
//! 3. 清除所有边框样式
//! 4. SetParent 嵌入
//! 5. 验证 NC 偏移，必要时 MoveWindow 补偿

use anyhow::{bail, Result};
use log::{debug, info, warn};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowExW, SetParent};

use super::encode_wide;
use super::strategy::{EmbedStrategy, MonitorRect};
use super::workerw::find_workerw_classic;
use super::wndproc::{register_subclass, subclass_wndproc};

// ============================================================
// NC 偏移测量（仅 ModernStrategy 使用）
// ============================================================

/// NC 偏移测量结果
struct NcOffset {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    client_w: i32,
    client_h: i32,
}

impl NcOffset {
    fn has_offset(&self) -> bool {
        self.left != 0 || self.top != 0 || self.right != 0 || self.bottom != 0
    }
}

/// 测量窗口的 NC（非客户区）偏移
unsafe fn measure_nc_offset(hwnd: HWND) -> NcOffset {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowRect};

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
// ModernStrategy 实现
// ============================================================

/// 24H2+ 嵌入策略
pub(super) struct ModernStrategy;

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
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, MoveWindow, SetLayeredWindowAttributes,
            SetWindowLongPtrW, SetWindowPos,
            GWL_EXSTYLE, GWL_STYLE, GWL_WNDPROC, LWA_ALPHA,
            SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
            WS_BORDER, WS_CAPTION, WS_CHILD, WS_DLGFRAME,
            WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME, WS_EX_LAYERED,
            WS_EX_STATICEDGE, WS_EX_TRANSPARENT, WS_EX_WINDOWEDGE,
            WS_THICKFRAME, WS_VISIBLE,
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
