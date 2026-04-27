use tauri::Manager;

mod commands;
mod db;
mod entities;
mod migration;
mod platform;
mod services;
mod utils;

use utils::timer_registry::{self, TimerRegistryState};
use services::wallpaper_window_service;
use platform::tray;

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

            app.manage(db);

            // 注入 TimerRegistry state（全局唯一定时器管理）
            let timer_registry = timer_registry::create_timer_registry();
            app.manage(timer_registry);

            // 注入 WallpaperWindowManager state
            let ww_manager = wallpaper_window_service::create_wallpaper_window_manager();
            app.manage(ww_manager);

            // ===== 系统托盘 =====
            tray::setup_tray(app)?;

            Ok(())
        })
        .invoke_handler(commands::all_handlers!())
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { .. } = &event {
                // 统一停止所有定时器（轮播 + 全屏检测等）
                if let Some(registry_state) = app.try_state::<TimerRegistryState>() {
                    let registry_state = registry_state.inner().clone();
                    tauri::async_runtime::block_on(async {
                        let mut registry = registry_state.lock().await;
                        registry.stop_all();
                    });
                }
            }
        });
}
