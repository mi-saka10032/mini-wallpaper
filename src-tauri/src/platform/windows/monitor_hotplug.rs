//! 显示器热插拔监听（M20）
//!
//! 在 Tauri 主窗口上挂 WndProc 子类化，拦截 `WM_DISPLAYCHANGE` / `WM_DPICHANGED`，
//! 通过 mpsc 转给异步侧做 debounce + 显示器重新枚举。
//!
//! # 为什么是主窗口而不是壁纸窗口
//!
//! `WM_DISPLAYCHANGE` 是广播消息，发给所有顶层窗口；主窗口是应用级单例，
//! 生命周期跟随应用，卸载必定伴随 WndProc 注销，不存在"未注册窗口期"。
//! 壁纸窗口会被 SetParent 到 WorkerW、按 backend 反复重建，不适合做事件源。
//!
//! # 同步回调的纪律
//!
//! WndProc 是同步 Win32 回调，**绝不能**在里面 await / 查 DB / 持 tokio 锁。
//! 这里只做 `try_send(())` 投信，业务逻辑全部在异步消费侧。

use log::{debug, warn};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, DefWindowProcW, SetWindowLongPtrW, GWLP_WNDPROC, WM_DISPLAYCHANGE,
    WM_DPICHANGED,
};

use super::desktop_embedder::wndproc::{
    find_original_wndproc, register_subclass, restore_and_unregister,
};

/// 投信器：热插拔消息到达时只做 `try_send`，不阻塞 UI 线程。
static HOTPLUG_TX: OnceLock<Mutex<Option<mpsc::UnboundedSender<()>>>> = OnceLock::new();

/// 已子类化的主窗口 HWND（存 isize：HWND 是裸指针、非 Send/Sync，不能进 static），
/// 用于应用退出时注销。还原成 HWND 时再 as 转回。
static SUBCLASSED_HWND: OnceLock<Mutex<Option<isize>>> = OnceLock::new();

/// 启动监听：把主窗口 WndProc 换成 `hotplug_wndproc`，并把投信器存入全局。
///
/// 必须在主窗口创建后、事件循环启动前调用。重复调用会先注销旧子类化，
/// 避免 HWND 复用后残留表项导致野指针跳转。
pub fn start(app: &AppHandle, tx: mpsc::UnboundedSender<()>) -> bool {
    let Some(window) = app.get_webview_window("main") else {
        warn!("[MonitorHotplug] 主窗口不存在，监听未启动");
        return false;
    };
    let Ok(hwnd) = window.hwnd() else {
        warn!("[MonitorHotplug] 拿不到主窗口 HWND，监听未启动");
        return false;
    };
    let hwnd = hwnd.0 as HWND;

    // 幂等：先注销可能存在的旧子类化。
    stop();

    unsafe {
        // 先转裸指针再转整数：直接 fn item → isize 会触发 function_casts_as_integer lint，
        // 且语义上 WndProc 就是函数指针，两步转换更准确。
        let original = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, hotplug_wndproc as *const () as isize);
        if original == 0 {
            warn!("[MonitorHotplug] SetWindowLongPtrW 失败，监听未启动");
            return false;
        }
        register_subclass(hwnd, original);
    }

    *HOTPLUG_TX
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("hotplug tx lock poisoned") = Some(tx);
    *SUBCLASSED_HWND
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("hotplug hwnd lock poisoned") = Some(hwnd as isize);

    debug!(
        "[MonitorHotplug] 已挂载主窗口 WndProc 子类化: hwnd=0x{:X}",
        hwnd as isize
    );
    true
}

/// 注销监听：还原原始 WndProc 并清理映射。必须在主窗口销毁前调用。
pub fn stop() {
    let hwnd = SUBCLASSED_HWND
        .get()
        .and_then(|lock| lock.lock().ok())
        .and_then(|guard| *guard);
    if let Some(hwnd) = hwnd {
        restore_and_unregister(hwnd as HWND);
        *SUBCLASSED_HWND
            .get()
            .and_then(|lock| lock.lock().ok())
            .expect("hotplug hwnd lock poisoned") = None;
    }
    if let Some(lock) = HOTPLUG_TX.get() {
        if let Ok(mut guard) = lock.lock() {
            *guard = None;
        }
    }
}

/// 主窗口 WndProc：只处理热插拔消息，其余原样转发给 tao。
unsafe extern "system" fn hotplug_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_DISPLAYCHANGE || msg == WM_DPICHANGED {
        if let Some(lock) = HOTPLUG_TX.get() {
            if let Ok(guard) = lock.lock() {
                if let Some(tx) = guard.as_ref() {
                    // unbounded channel：send 不会阻塞。消息无 payload，
                    // 消费侧 debounce 后重新枚举全量显示器。
                    let _ = tx.send(());
                }
            }
        }
    }

    if let Some(original_proc) = find_original_wndproc(hwnd) {
        type WndProcFn = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;
        let fn_ptr: WndProcFn = std::mem::transmute(original_proc as usize);
        CallWindowProcW(Some(fn_ptr), hwnd, msg, wparam, lparam)
    } else {
        // 与壁纸窗口 subclass 相同的兜底：映射缺失说明本窗口未被我们子类化，
        // 用 DefWindowProcW 保证消息至少被系统默认处理，不静默吞掉。
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}
