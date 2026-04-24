//! WndProc 子类化基础设施
//!
//! 仅 24H2+ ModernStrategy 使用。
//! 通过替换窗口过程拦截 WM_NCCALCSIZE 消息，强制 NC 区域为 0，
//! 消除 DWM 在 SetParent 后注入的隐藏边框。
//!
//! 全局槽位设计支持最多 8 个显示器同时子类化。

use log::{debug, warn};
use std::sync::atomic::{AtomicIsize, Ordering};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};

/// 子类化槽位：保存原始 WndProc（支持最多 8 个显示器）
static ORIGINAL_WNDPROCS: [AtomicIsize; 8] = [
    AtomicIsize::new(0), AtomicIsize::new(0), AtomicIsize::new(0), AtomicIsize::new(0),
    AtomicIsize::new(0), AtomicIsize::new(0), AtomicIsize::new(0), AtomicIsize::new(0),
];

/// 子类化槽位：保存对应的 HWND
static SUBCLASSED_HWNDS: [AtomicIsize; 8] = [
    AtomicIsize::new(0), AtomicIsize::new(0), AtomicIsize::new(0), AtomicIsize::new(0),
    AtomicIsize::new(0), AtomicIsize::new(0), AtomicIsize::new(0), AtomicIsize::new(0),
];

/// 根据 HWND 查找已注册的原始 WndProc
fn find_original_wndproc(hwnd: HWND) -> Option<isize> {
    let hwnd_val = hwnd as isize;
    SUBCLASSED_HWNDS
        .iter()
        .zip(ORIGINAL_WNDPROCS.iter())
        .find_map(|(h, p)| {
            if h.load(Ordering::SeqCst) == hwnd_val {
                let proc = p.load(Ordering::SeqCst);
                (proc != 0).then_some(proc)
            } else {
                None
            }
        })
}

/// 注册子类化信息到空闲槽位
pub(super) fn register_subclass(hwnd: HWND, original_proc: isize) {
    let hwnd_val = hwnd as isize;
    for (i, (h, p)) in SUBCLASSED_HWNDS
        .iter()
        .zip(ORIGINAL_WNDPROCS.iter())
        .enumerate()
    {
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
pub(super) unsafe extern "system" fn subclass_wndproc(
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
