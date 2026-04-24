use sea_orm::DatabaseConnection;
use log::info;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;

mod commands;
mod db;
mod entities;
mod migration;
mod platform;
mod services;
mod utils;

use services::timer_manager::{self, TimerManagerState};
use services::wallpaper_window_service;
use platform::fullscreen_detector::FullscreenDetector;

/// 全屏检测器 managed state
pub type FullscreenDetectorState = std::sync::Arc<tokio::sync::Mutex<FullscreenDetector>>;

/// 全局应用状态，持有数据库连接
pub struct AppState {
    pub db: DatabaseConnection,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            // 初始化日志系统
            env_logger::Builder::from_env(
                env_logger::Env::default().default_filter_or("info")
            ).init();

            let handle = app.handle().clone();

            // 同步初始化数据库（release 模式下 <50ms）
            let db = tauri::async_runtime::block_on(async { db::init_db(&handle).await })
                .expect("Failed to initialize database");

            app.manage(AppState { db });

            // 注入 TimerManager state
            let timer_manager = timer_manager::create_timer_manager();
            app.manage(timer_manager);

            // 注入 WallpaperWindowManager state
            let ww_manager = wallpaper_window_service::create_wallpaper_window_manager();
            app.manage(ww_manager);

            // ===== 全屏检测器（注入空 state，异步延迟启动）=====
            let detector_state: FullscreenDetectorState =
                std::sync::Arc::new(tokio::sync::Mutex::new(FullscreenDetector::new()));
            app.manage(detector_state.clone());

            // setup 结束后异步读 DB 决定是否启动，不阻塞窗口渲染
            let deferred_handle = app.handle().clone();
            let deferred_detector = detector_state.clone();
            let deferred_db = {
                let state: tauri::State<'_, AppState> = app.state();
                state.db.clone()
            };
            tauri::async_runtime::spawn(async move {
                let should_start = services::app_setting_service::get(&deferred_db, "pause_on_fullscreen")
                    .await
                    .unwrap_or(None)
                    .map(|v| v == "true")
                    .unwrap_or(false);
                if should_start {
                    let mut detector = deferred_detector.lock().await;
                    detector.start(deferred_handle);
                }
            });

            // ===== 系统托盘 =====
            let show_i = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_i, &quit_i])?;

            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Mini Wallpaper")
                .menu(&tray_menu)
                .menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.unminimize();
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.unminimize();
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::wallpaper::get_wallpapers,
            commands::wallpaper::import_wallpapers,
            commands::wallpaper::delete_wallpapers,
            commands::collection::get_collections,
            commands::collection::create_collection,
            commands::collection::rename_collection,
            commands::collection::delete_collection,
            commands::collection::get_collection_wallpapers,
            commands::collection::add_wallpapers_to_collection,
            commands::collection::remove_wallpapers_from_collection,
            commands::collection::reorder_collection_wallpapers,
            commands::monitor_config::get_monitor_configs,
            commands::monitor_config::get_monitor_config,
            commands::monitor_config::upsert_monitor_config,
            commands::monitor_config::delete_monitor_config,
            commands::app_setting::get_settings,
            commands::app_setting::get_setting,
            commands::app_setting::set_setting,
            commands::shortcut::switch_wallpaper,
            commands::backup::export_backup,
            commands::backup::import_backup,
            commands::backup::get_data_size,
            commands::fullscreen::set_fullscreen_detection,
            commands::wallpaper_window::create_wallpaper_window,
            commands::wallpaper_window::destroy_wallpaper_window,
            commands::wallpaper_window::destroy_all_wallpaper_windows,
            commands::wallpaper_window::hide_wallpaper_windows,
            commands::wallpaper_window::show_wallpaper_windows,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            if let tauri::RunEvent::ExitRequested { api, code, .. } = &event {
                // 停止所有定时器
                if let Some(timer_state) = app.try_state::<TimerManagerState>() {
                    let timer_state = timer_state.inner().clone();
                    tauri::async_runtime::block_on(async {
                        let mut manager = timer_state.lock().await;
                        manager.stop_all();
                    });
                }
                // code 有值 = 显式 exit() 调用，放行退出
                // code 为 None = 最后一个窗口关闭等隐式退出，阻止退出保持后台
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}