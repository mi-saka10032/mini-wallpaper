use sea_orm::DatabaseConnection;
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
use platform::tray;

/// 全屏检测器 managed state
pub type FullscreenDetectorState = std::sync::Arc<tokio::sync::Mutex<FullscreenDetector>>;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        // MacosLauncher 为 API 必需参数，仅 macOS 生效；本项目仅面向 Windows，此参数无实际作用
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

            app.manage(db);

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
                let state: tauri::State<'_, DatabaseConnection> = app.state();
                state.inner().clone()
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
            tray::setup_tray(app)?;

            Ok(())
        })
        .invoke_handler(commands::all_handlers!())
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { .. } = &event {
                // 停止所有定时器
                if let Some(timer_state) = app.try_state::<TimerManagerState>() {
                    let timer_state = timer_state.inner().clone();
                    tauri::async_runtime::block_on(async {
                        let mut manager = timer_state.lock().await;
                        manager.stop_all();
                    });
                }
            }
        });
}