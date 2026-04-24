//! 旧版本嵌入策略（LegacyStrategy）
//!
//! 适用于 Win7 ~ Win11 23H2。
//! 经典方案：EnumWindows 查找 SHELLDLL_DefView 所在 WorkerW 的前一个兄弟，
//! 简单 SetParent + MoveWindow，无需额外修复。

use anyhow::{bail, Result};
use log::info;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::SetParent;

use super::workerw::find_workerw_classic;
use super::{EmbedStrategy, MonitorRect};

/// 旧版本嵌入策略
pub(super) struct LegacyStrategy;

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
