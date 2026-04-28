mod commands;
mod ctx;
mod dto;
mod entities;
mod migration;
mod platform;
mod runtime;
mod services;
mod utils;

use std::sync::Arc;

use tauri::Manager;
use tokio::sync::Mutex;

use ctx::AppContext;
use platform::tray;
use runtime::Scheduler;

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

            // 构造 AppContext（内部完成 DB 初始化）并注册为全局 state
            let ctx = tauri::async_runtime::block_on(async {
                AppContext::new(handle.clone()).await
            })
            .expect("Failed to initialize AppContext");
            app.manage(ctx);

            // 构造 Scheduler（独立全局 state，纯 JoinHandle 注册表）
            let scheduler = Arc::new(Mutex::new(Scheduler::new()));
            app.manage(scheduler);

            // ===== 系统托盘 =====
            tray::setup_tray(app)?;

            Ok(())
        })
        .invoke_handler(commands::all_handlers!())
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { .. } = &event {
                // 统一停止调度器内所有后台任务（轮播 + 全屏检测等）
                if let Some(scheduler) = app.try_state::<Arc<Mutex<Scheduler>>>() {
                    let scheduler = scheduler.inner().clone();
                    tauri::async_runtime::block_on(async {
                        let mut sched = scheduler.lock().await;
                        sched.stop_all();
                    });
                }
            }
        });
}