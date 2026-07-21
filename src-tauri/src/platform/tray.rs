//! 系统托盘图标、菜单及交互事件
//!
//! 托盘菜单是 [`Action`] 体系的第三个触发源（与全局快捷键、前端命令并列）。
//! 菜单项点击统一走 [`Scheduler::dispatch_action`]，保证行为一致。
//!
//! ## 菜单结构
//!
//! ```text
//! ▶ 下一张          → Action::Next
//! ◀ 上一张          → Action::Prev
//! ⏸ 暂停 / ▶ 恢复   → Action::TogglePause（文本动态切换）
//! ❤ 收藏当前壁纸    → Action::ToggleFavorite
//! ─── 分隔线 ───
//! 🖥 显示主窗口      → Action::OpenMain
//! ─── 分隔线 ───
//! ❌ 退出            → Action::Quit
//! ```
//!
//! ## 左键单击托盘图标
//!
//! 直接显示并聚焦主窗口（等同 OpenMain）。

use std::sync::Arc;

use log::{info, warn};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;
use tokio::sync::Mutex;

use crate::runtime::action::Action;
use crate::runtime::Scheduler;

// ==================== 菜单项 ID 常量 ====================

const ID_NEXT: &str = "tray_next";
const ID_PREV: &str = "tray_prev";
const ID_TOGGLE_PAUSE: &str = "tray_toggle_pause";
const ID_TOGGLE_FAVORITE: &str = "tray_toggle_favorite";
const ID_OPEN_MAIN: &str = "tray_open_main";
const ID_QUIT: &str = "tray_quit";

// ==================== 公共入口 ====================

/// 初始化系统托盘图标、菜单及交互事件
///
/// 在 `lib.rs` 的 `setup` 阶段调用。
pub fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // ===== 构建菜单项 =====
    let next_i = MenuItem::with_id(app, ID_NEXT, "下一张壁纸", true, None::<&str>)?;
    let prev_i = MenuItem::with_id(app, ID_PREV, "上一张壁纸", true, None::<&str>)?;
    let pause_i = MenuItem::with_id(app, ID_TOGGLE_PAUSE, "暂停轮播", true, None::<&str>)?;
    let favorite_i = MenuItem::with_id(app, ID_TOGGLE_FAVORITE, "收藏当前壁纸", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let open_main_i = MenuItem::with_id(app, ID_OPEN_MAIN, "显示主窗口", true, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit_i = MenuItem::with_id(app, ID_QUIT, "退出", true, None::<&str>)?;

    let tray_menu = Menu::with_items(
        app,
        &[&next_i, &prev_i, &pause_i, &favorite_i, &sep1, &open_main_i, &sep2, &quit_i],
    )?;

    // ===== 构建托盘 =====
    TrayIconBuilder::new()
        .icon(
            app.default_window_icon()
                .expect("default icon must exist")
                .clone(),
        )
        .tooltip("Mini Wallpaper")
        .menu(&tray_menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let action = match event.id.as_ref() {
                ID_NEXT => Action::Next,
                ID_PREV => Action::Prev,
                ID_TOGGLE_PAUSE => Action::TogglePause,
                ID_TOGGLE_FAVORITE => Action::ToggleFavorite,
                ID_OPEN_MAIN => Action::OpenMain,
                ID_QUIT => Action::Quit,
                _ => return,
            };
            dispatch_tray_action(app, action);
        })
        .on_tray_icon_event(|tray, event| {
            // 左键单击托盘图标 → 显示主窗口
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

// ==================== 公共工具函数 ====================

/// 显示并聚焦主窗口（供 dispatcher 和托盘事件复用）
pub fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

// ==================== 内部工具 ====================

/// 异步派发 Action 到 Scheduler（与 global_shortcut 中的 dispatch_async 同构）
fn dispatch_tray_action(app: &tauri::AppHandle, action: Action) {
    info!("[Tray] 触发动作: {:?}", action);
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let scheduler_state = match app_handle.try_state::<Arc<Mutex<Scheduler>>>() {
            Some(s) => s,
            None => {
                warn!("[Tray] Scheduler 尚未注册，丢弃动作 {:?}", action);
                return;
            }
        };
        let scheduler = scheduler_state.inner().clone();
        let mut sched = scheduler.lock().await;
        sched.dispatch_action(action).await;
    });
}