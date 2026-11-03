use std::sync::Arc;

use tauri::Manager;
use tokio::sync::Mutex;

use crate::ctx::AppContext;
use crate::platform::{global_shortcut, tray::register_tray};
use crate::runtime::Scheduler;

pub const AUTOSTART_ARG: &str = "--autostart";

/// 第二个实例启动时，聚焦已有窗口。
pub fn focus_existing_window(app: &tauri::AppHandle, _args: Vec<String>, _cwd: String) {
    let window: tauri::WebviewWindow = app
        .get_webview_window("main")
        .expect("Main window not found");
    window
        .unminimize()
        .expect("Failed to unminimize main window");
    window.show().expect("Failed to show main window");
    window
        .set_focus()
        .expect("Failed to set focus on main window");
}

pub fn init(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    register_logger(app)?;
    register_runtime_state(app)?;
    register_tray(app)?;
    register_shortcuts(app);
    hide_main_on_autostart(app)?;
    spawn_trash_purge(app);
    Ok(())
}

/// `tauri dev` 是 debug 构建，日志走控制台，默认 info，可用 `RUST_LOG` 覆盖。
/// `tauri build` 是 release，启动时创建 `logs/error.log`，只追加 error。
/// 目录或文件创建失败时退回控制台，不阻止应用启动。
fn register_logger(app: &tauri::App) -> tauri::Result<()> {
    let is_dev = cfg!(debug_assertions);

    if !is_dev {
        let open_release_log = || -> Option<std::fs::File> {
            let app_data_dir = app.handle().path().app_data_dir().ok()?;
            let log_dir = app_data_dir.join("logs");
            std::fs::create_dir_all(&log_dir).ok()?;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_dir.join("error.log"))
                .ok()
        };

        if let Some(file) = open_release_log() {
            env_logger::Builder::new()
                .target(env_logger::Target::Pipe(Box::new(file)))
                .filter_level(log::LevelFilter::Error)
                .init();
            return Ok(());
        }
    }

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    Ok(())
}

/// 注册 AppContext 与 Scheduler。数据库连接在 AppContext 内部完成。
fn register_runtime_state(app: &mut tauri::App) -> tauri::Result<()> {
    let handle = app.handle();

    let ctx: AppContext =
        tauri::async_runtime::block_on(async { AppContext::new(handle.clone()).await })?;
    app.manage(ctx);

    let scheduler: Arc<Mutex<Scheduler>> = Arc::new(Mutex::new(Scheduler::new(handle.clone())));
    app.manage(scheduler);
    Ok(())
}

/// 开机自启时主窗口直接进托盘。
fn hide_main_on_autostart(app: &tauri::App) -> tauri::Result<()> {
    if !std::env::args().any(|arg| arg == AUTOSTART_ARG) {
        return Ok(());
    }
    app.get_webview_window("main")
        .ok_or(tauri::Error::WindowNotFound)?
        .hide()?;
    Ok(())
}

/// 快捷键要读数据库键位，并派发到 Scheduler，所以放在两者注册之后。
fn register_shortcuts(app: &tauri::App) {
    let app_handle = app.handle().clone();
    tauri::async_runtime::block_on(async move {
        global_shortcut::register_default_shortcuts(&app_handle).await;
    });
}

/// 启动后扫一次过期回收站，不阻塞启动。失败只记日志。
fn spawn_trash_purge(app: &tauri::App) {
    let app_handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        crate::services::trash_maintenance::purge_expired_on_startup(&app_handle).await;
    });
}
