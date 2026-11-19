use std::sync::OnceLock;

use anyhow::{bail, Result};
use windows::core::w;
use windows::Win32::Foundation::{
    GetLastError, ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowLongPtrW, LoadCursorW, RegisterClassW,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, CS_HREDRAW, CS_VREDRAW, GWL_EXSTYLE, HICON,
    IDC_ARROW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_SHOWNA,
    WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
    WS_POPUP,
};

use crate::platform::wallpaper_renderer::MonitorRect;

const CLASS_NAME: windows::core::PCWSTR = w!("MiniWallpaperNativeRenderer");
const WINDOW_NAME: windows::core::PCWSTR = w!("MiniWallpaperSurface");

pub struct NativeWindow {
    hwnd: HWND,
}

impl NativeWindow {
    pub fn new(rect: MonitorRect) -> Result<Self> {
        ensure_class_registered()?;
        let instance = unsafe { GetModuleHandleW(None)? };

        // WS_EX_NOREDIRECTIONBITMAP 是创建期样式，DirectComposition 必须在窗口创建时就声明，
        // 不能等 SetParent 后再补；否则 HWND 的 GDI 重定向位图仍会参与 24H2 桌面合成链。
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                CLASS_NAME,
                WINDOW_NAME,
                WS_POPUP,
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                None,
                None,
                Some(instance.into()),
                None,
            )?
        };

        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNA);
        }

        Ok(Self { hwnd })
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// 嵌入后补一次显示。SW_SHOWNA 幂等，不会影响已显示的窗口。
    pub fn show(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNA);
        }
    }

    /// 原生窗口专用的扩展样式归一，必须在 embed 之后执行——嵌入策略会重设 exstyle。
    ///
    /// 共享嵌入策略会给所有窗口加 `WS_EX_TRANSPARENT`（WebView 的鼠标穿透需要）。
    /// 原生窗口走 DirectComposition 合成，不需要该位；创建期带的
    /// `WS_EX_NOACTIVATE` / `WS_EX_TOOLWINDOW` 属于顶层窗口语义，成为 WorkerW
    /// 子窗口后一并清掉。唯一必须保留的是 `WS_EX_NOREDIRECTIONBITMAP`：它是创建期
    /// 样式，DirectComposition target 依赖窗口不建立 GDI 重定向位图。
    pub fn normalize_native_ex_style(&self) -> Result<()> {
        unsafe {
            let old_ex_style = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            let new_ex_style = old_ex_style
                & !(WS_EX_TRANSPARENT.0 as isize)
                & !(WS_EX_NOACTIVATE.0 as isize)
                & !(WS_EX_TOOLWINDOW.0 as isize);

            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, new_ex_style);
            SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            )?;

            let applied_ex_style = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            if applied_ex_style != new_ex_style {
                bail!("子窗口扩展样式归一失败");
            }
        }
        Ok(())
    }

    pub fn set_bounds(&self, rect: MonitorRect) -> Result<()> {
        unsafe {
            SetWindowPos(
                self.hwnd,
                None,
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )?;
        }
        Ok(())
    }
}

impl Drop for NativeWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn ensure_class_registered() -> Result<()> {
    static REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();

    let result = REGISTERED.get_or_init(|| {
        let instance = unsafe { GetModuleHandleW(None).map_err(|e| e.to_string())? };
        let cursor = unsafe { LoadCursorW(None, IDC_ARROW).map_err(|e| e.to_string())? };
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance.into(),
            hIcon: HICON::default(),
            hCursor: cursor,
            hbrBackground: HBRUSH::default(),
            lpszMenuName: windows::core::PCWSTR::null(),
            lpszClassName: CLASS_NAME,
        };

        let atom = unsafe { RegisterClassW(&class) };
        if atom == 0 {
            let err = unsafe { GetLastError() };
            if err == ERROR_CLASS_ALREADY_EXISTS {
                return Ok(());
            }
            return Err(format!("RegisterClassW failed: {:?}", err));
        }

        Ok(())
    });

    match result {
        Ok(()) => Ok(()),
        Err(message) => bail!("{}", message),
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}
