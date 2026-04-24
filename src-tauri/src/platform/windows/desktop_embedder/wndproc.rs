//! WndProc 子类化基础设施
//!
//! 仅 24H2+ ModernStrategy 使用。
//! 通过替换窗口过程拦截 WM_NCCALCSIZE 消息，强制 NC 区域为 0，
//! 消除 DWM 在 SetParent 后注入的隐藏边框。
//!
//! 使用 HashMap 动态管理子类化映射，无显示器数量上限。

use log::{debug, warn};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};

/// 全局子类化映射表：HWND → 原始 WndProc 地址
///
/// OnceLock 保证惰性初始化且线程安全，Mutex 保护内部 HashMap 的并发访问。
/// 相比原先的 [AtomicIsize; 8] 双数组方案：
/// - 无硬编码槽位上限
/// - O(1) 查找替代 O(n) 线性扫描
/// - 代码更简洁，语义更清晰
fn subclass_map() -> &'static Mutex<HashMap<isize, isize>> {
    static MAP: OnceLock<Mutex<HashMap<isize, isize>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 根据 HWND 查找已注册的原始 WndProc
fn find_original_wndproc(hwnd: HWND) -> Option<isize> {
    subclass_map()
        .lock()
        .ok()
        .and_then(|map| map.get(&(hwnd as isize)).copied())
}

/// 注册子类化信息：记录 HWND 与其原始 WndProc 的映射
pub(super) fn register_subclass(hwnd: HWND, original_proc: isize) {
    let hwnd_val = hwnd as isize;
    match subclass_map().lock() {
        Ok(mut map) => {
            map.insert(hwnd_val, original_proc);
            debug!(
                "子类化已注册: hwnd=0x{:X}, orig_proc=0x{:X}, 当前数量={}",
                hwnd_val, original_proc, map.len()
            );
        }
        Err(e) => {
            warn!("子类化注册失败（锁中毒）: hwnd=0x{:X}, err={}", hwnd_val, e);
        }
    }
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
