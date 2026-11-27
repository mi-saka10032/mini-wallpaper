//! 系统托盘图标、菜单及交互事件
//!
//! 托盘菜单是 [`Action`] 体系的第三个触发源（与全局快捷键、前端命令并列）。
//! 菜单项点击统一走 `application::actions::dispatch_action`，保证行为一致。
//!
//! ## 动态场景子菜单
//!
//! 场景的名称与数量由用户随时增删改，因此托盘菜单必须能在启动后更新。
//! Tauri 2 提供 `TrayIcon::set_menu`：按最新列表重新构造一份 `Menu` 再替换即可，
//! **不需要销毁并重建托盘图标**，图标与全局 `on_menu_event` 处理器都保持原样。
//!
//! 注意 Linux 文档标注「菜单一旦设置不可替换、只能改内容」；本项目是
//! Windows-only，走 set_menu 没问题，若将来跨平台需改为原地 append/remove。
//!
//! 场景项 id 形如 `scene_<id>`，在全局 handler 的兜底分支里解析；
//! 其它窗口的菜单事件同样会经过该 handler，必须静默忽略而非误解析。

use std::sync::Arc;

use log::{info, warn};
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use tokio::sync::Mutex;

use crate::application::{self, AppServices};
use crate::context::AppContext;
use crate::dto::action::Action;
use crate::dto::scene_dto::SceneSummary;
use crate::services::scene_service;
use crate::tasks::registry::TaskRegistry;

// ==================== 常量 ====================

/// 固定托盘 id：运行时要靠它取回 handle 更新菜单。
pub const TRAY_ID: &str = "main";

/// 托盘里最多列出的场景数，超出时给一个禁用的提示项。
const MAX_TRAY_SCENES: usize = 10;

const SCENE_PREFIX: &str = "scene_";
const ID_NEXT: &str = "tray_next";
const ID_PREV: &str = "tray_prev";
const ID_TOGGLE_PAUSE: &str = "tray_toggle_pause";
const ID_TOGGLE_FAVORITE: &str = "tray_toggle_favorite";
const ID_OPEN_MAIN: &str = "tray_open_main";
const ID_QUIT: &str = "tray_quit";
const ID_SCENE_BACK: &str = "tray_scene_back";
const ID_SCENE_EMPTY: &str = "tray_scene_empty";
const ID_SCENE_MORE: &str = "tray_scene_more";

// ==================== 公共入口 ====================

/// 注册托盘图标与初始菜单；`setup::init` 中 `register_runtime_state` 已先执行，DB 可用。
pub fn register_tray(app: &tauri::App) -> tauri::Result<()> {
    let icon = app
        .default_window_icon()
        .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".into()))?
        .clone();

    let ctx = app.state::<Arc<AppContext>>();
    let rows = tauri::async_runtime::block_on(async {
        scene_service::get_all(&ctx.inner().db)
            .await
            .unwrap_or_default()
    });
    let summaries = application::scene::list_scenes_from_db(rows);
    // build_tray_menu 收 AppHandle；App 与 AppHandle 都实现 Manager，这里取一次 handle。
    let handle = app.handle();
    let tray_menu = build_tray_menu(&handle, &summaries, ctx.inner().last_scene_id())?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Mini Wallpaper")
        .menu(&tray_menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let id = event.id.as_ref();
            let action = match id {
                ID_NEXT => Action::Next,
                ID_PREV => Action::Prev,
                ID_TOGGLE_PAUSE => Action::TogglePause,
                ID_TOGGLE_FAVORITE => Action::ToggleFavorite,
                ID_OPEN_MAIN => Action::OpenMain,
                ID_QUIT => Action::Quit,
                ID_SCENE_BACK => Action::ApplyLastScene,
                _ => {
                    // 动态场景项；其它窗口的菜单事件也会到这里，一律静默忽略。
                    let Some(scene_id) = id
                        .strip_prefix(SCENE_PREFIX)
                        .and_then(|raw| raw.parse::<i32>().ok())
                    else {
                        return;
                    };
                    Action::ApplyScene { scene_id }
                }
            };
            dispatch_tray_action(app, action);
        })
        .on_tray_icon_event(|tray, event| {
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

/// 按最新场景列表重建菜单并替换（不重建托盘图标、不重绑事件）。
///
/// 由 `application::scene` 在保存 / 重命名 / 删除 / 应用场景成功后单点调用。
pub fn refresh_tray_menu(app: &AppHandle, scenes: &[SceneSummary], last_scene_id: Option<i32>) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        warn!("[Tray] 托盘尚未注册，跳过场景菜单刷新");
        return;
    };
    match build_tray_menu(app, scenes, last_scene_id) {
        Ok(menu) => {
            if let Err(e) = tray.set_menu(Some(menu)) {
                warn!("[Tray] 更新场景菜单失败: {}", e);
            }
        }
        Err(e) => warn!("[Tray] 构造场景菜单失败: {}", e),
    }
}

// ==================== 菜单构造 ====================

fn build_tray_menu(
    app: &AppHandle,
    scenes: &[SceneSummary],
    last_scene_id: Option<i32>,
) -> tauri::Result<Menu<Wry>> {
    let next_i = MenuItem::with_id(app, ID_NEXT, "下一张壁纸", true, None::<&str>)?;
    let prev_i = MenuItem::with_id(app, ID_PREV, "上一张壁纸", true, None::<&str>)?;
    let pause_i = MenuItem::with_id(app, ID_TOGGLE_PAUSE, "暂停/恢复轮播", true, None::<&str>)?;
    let favorite_i =
        MenuItem::with_id(app, ID_TOGGLE_FAVORITE, "收藏当前壁纸", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let open_main_i = MenuItem::with_id(app, ID_OPEN_MAIN, "显示主窗口", true, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit_i = MenuItem::with_id(app, ID_QUIT, "退出", true, None::<&str>)?;

    // 动态项由 Vec 持有所有权，再取引用交给 Submenu::with_items。
    let mut scene_items: Vec<MenuItem<Wry>> = Vec::new();
    for scene in scenes.iter().take(MAX_TRAY_SCENES) {
        scene_items.push(MenuItem::with_id(
            app,
            format!("{SCENE_PREFIX}{}", scene.id),
            scene.name.as_str(),
            true,
            None::<&str>,
        )?);
    }

    let back_text = last_scene_id
        .and_then(|id| scenes.iter().find(|s| s.id == id))
        .map(|s| format!("返回：{}", s.name))
        .unwrap_or_else(|| "返回上一个场景".to_string());
    let back_i = MenuItem::with_id(
        app,
        ID_SCENE_BACK,
        back_text.as_str(),
        last_scene_id.is_some(),
        None::<&str>,
    )?;
    let empty_i = MenuItem::with_id(app, ID_SCENE_EMPTY, "（暂无场景）", false, None::<&str>)?;
    let more_i = MenuItem::with_id(
        app,
        ID_SCENE_MORE,
        "更多场景请在应用中管理",
        false,
        None::<&str>,
    )?;
    let sep_scene = PredefinedMenuItem::separator(app)?;

    let mut sub_items: Vec<&dyn IsMenuItem<Wry>> = vec![&back_i, &sep_scene];
    for item in &scene_items {
        sub_items.push(item);
    }
    if scene_items.is_empty() {
        sub_items.push(&empty_i);
    }
    if scenes.len() > MAX_TRAY_SCENES {
        sub_items.push(&more_i);
    }
    let scene_submenu = Submenu::with_items(app, "场景", true, &sub_items)?;

    Menu::with_items(
        app,
        &[
            &next_i,
            &prev_i,
            &pause_i,
            &favorite_i,
            &sep1,
            &open_main_i,
            &sep2,
            &scene_submenu,
            &quit_i,
        ],
    )
}

// ==================== 公共工具函数 ====================

/// 显示并聚焦主窗口（供 dispatcher 和托盘事件复用）
pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

// ==================== 内部工具 ====================

/// 异步派发 Action 到统一入口（与 global_shortcut 中的 dispatch_async 同构）
fn dispatch_tray_action(app: &AppHandle, action: Action) {
    info!("[Tray] 触发动作: {:?}", action);
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let scheduler_state = match app_handle.try_state::<Arc<Mutex<TaskRegistry>>>() {
            Some(s) => s,
            None => {
                warn!("[Tray] TaskRegistry 尚未注册，丢弃动作 {:?}", action);
                return;
            }
        };
        let ctx_state = match app_handle.try_state::<Arc<AppContext>>() {
            Some(s) => s,
            None => {
                warn!("[Tray] AppContext 尚未注册，丢弃动作 {:?}", action);
                return;
            }
        };
        let svc = AppServices::new(ctx_state.inner(), scheduler_state.inner());
        application::actions::dispatch_action(&svc, action).await;
    });
}
