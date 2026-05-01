mod commands;
mod ctx;
mod dto;
mod entities;
pub mod events;
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

/// 检查当前是否为开机自启动（通过命令行参数 --autostart 判断）
fn is_autostart_launch() -> bool {
    std::env::args().any(|arg| arg == "--autostart")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // 当第二个实例尝试启动时，聚焦已有窗口
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .setup(|app| {
            // 初始化日志系统
            env_logger::Builder::from_env(
                env_logger::Env::default().default_filter_or("info"),
            )
            .init();

            let handle = app.handle().clone();

            // 构造 AppContext（内部完成 DB 初始化）并注册为全局 state
            let ctx = tauri::async_runtime::block_on(async {
                AppContext::new(handle.clone()).await
            })
            .expect("Failed to initialize AppContext");
            app.manage(ctx);

            // 构造 Scheduler（持有 AppHandle，通过控制反转向任务注入句柄）
            let scheduler = Arc::new(Mutex::new(Scheduler::new(handle)));
            app.manage(scheduler);

            // ===== 系统托盘 =====
            tray::setup_tray(app)?;

            // ===== 开机自启时隐藏主窗口到托盘 =====
            if is_autostart_launch() {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }

            Ok(())
        })
        .invoke_handler(commands::all_handlers!())
        .build(tauri::generate_context!())
        .expect("error while building Mini Wallpaper")
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
