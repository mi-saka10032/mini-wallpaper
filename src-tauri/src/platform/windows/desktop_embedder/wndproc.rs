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
///
/// 提为 `pub(crate)` 供主窗口热插拔监听（M20）复用同一套映射表；
/// 两个消费方各自持有自己的 wndproc 函数，但共用 register/restore 生命周期。
pub(crate) fn find_original_wndproc(hwnd: HWND) -> Option<isize> {
    subclass_map()
        .lock()
        .ok()
        .and_then(|map| map.get(&(hwnd as isize)).copied())
}

/// 注册子类化信息：记录 HWND 与其原始 WndProc 的映射
pub(crate) fn register_subclass(hwnd: HWND, original_proc: isize) {
    let hwnd_val = hwnd as isize;
    match subclass_map().lock() {
        Ok(mut map) => {
            map.insert(hwnd_val, original_proc);
            debug!(
                "子类化已注册: hwnd=0x{:X}, orig_proc=0x{:X}, 当前数量={}",
                hwnd_val,
                original_proc,
                map.len()
            );
        }
        Err(e) => {
            warn!("子类化注册失败（锁中毒）: hwnd=0x{:X}, err={}", hwnd_val, e);
        }
    }
}

/// 还原原始窗口过程并注销映射，必须与 [`register_subclass`] 成对调用
///
/// 顺序是「先还原 WndProc，再移除映射」，反过来会留下一个窗口期：子类化仍挂在
/// 窗口上但映射已空，此间到达的消息在 [`subclass_wndproc`] 查不到原 proc，会被
/// 丢给 DefWindowProcW，绕过 tao 自己的窗口过程。
///
/// 还原与移除各自解决不同问题：
/// - 还原 WndProc —— 让后续的 WM_CLOSE / WM_DESTROY 正确回到 tao，窗口销毁
///   并非瞬时完成（见 WindowManager::create_window 对销毁延迟的处理）。
/// - 移除映射 —— HWND 被系统回收复用后，残留表项会让新窗口的消息 transmute
///   成已失效的函数指针并调用，即野指针跳转；同时避免 map 随窗口重建无界增长。
///
/// 未注册过该 HWND（LegacyStrategy 不做子类化）时静默返回。
pub(crate) fn restore_and_unregister(hwnd: HWND) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowLongPtrW, GWL_WNDPROC};

    let hwnd_val = hwnd as isize;
    let mut map = match subclass_map().lock() {
        Ok(map) => map,
        Err(e) => {
            warn!("子类化注销失败（锁中毒）: hwnd=0x{:X}, err={}", hwnd_val, e);
            return;
        }
    };

    let Some(&original_proc) = map.get(&hwnd_val) else {
        return;
    };

    // 持锁期间完成还原，确保不存在「已挂子类化但映射缺失」的中间态
    unsafe {
        SetWindowLongPtrW(hwnd, GWL_WNDPROC, original_proc);
    }
    map.remove(&hwnd_val);

    debug!(
        "子类化已注销: hwnd=0x{:X}, orig_proc=0x{:X}, 剩余数量={}",
        hwnd_val,
        original_proc,
        map.len()
    );
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
