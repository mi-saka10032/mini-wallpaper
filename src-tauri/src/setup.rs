use std::sync::Arc;

use log::warn;
use tauri::Manager;
use tokio::sync::Mutex;

use crate::ctx::AppContext;
use crate::platform::{global_shortcut, tray::register_tray};
use crate::runtime::Scheduler;

pub const AUTOSTART_ARG: &str = "--autostart";

/// 第二个实例启动时，聚焦已有窗口。
pub fn focus_existing_window(app: &tauri::AppHandle, _args: Vec<String>, _cwd: String) {
    let Some(window) = app.get_webview_window("main") else {
        warn!("[SingleInstance] 主窗口不存在，无法聚焦");
        return;
    };
    if let Err(error) = window.unminimize() {
        warn!("[SingleInstance] 取消最小化失败: {error}");
    }
    if let Err(error) = window.show() {
        warn!("[SingleInstance] 显示主窗口失败: {error}");
    }
    if let Err(error) = window.set_focus() {
        warn!("[SingleInstance] 聚焦主窗口失败: {error}");
    }
}

pub fn init(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    register_logger(app)?;
    register_runtime_state(app)?;
    register_tray(app)?;
    register_shortcuts(app);
    hide_main_on_autostart(app)?;
    spawn_trash_purge(app);
    start_monitor_hotplug(app);
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
        tauri::async_runtime::block_on(async { AppContext::new(handle.clone()).await }).map_err(
            |error| {
                log::error!("[Setup] 数据库初始化失败: {error:#}");
                error
            },
        )?;
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
///
/// 用 spawn 而非 block_on：注册所需的 AppContext / Scheduler / 快捷键 plugin
/// 在调用本函数时已全部就绪，后续 setup 步骤也不依赖「键位已注册」，
/// 因此无需阻塞启动。代价是进入事件循环后的极短窗口内按键可能无响应，
/// 注册完成后即恢复；单个键位失败仍由 register_default_shortcuts 内部 warn。
fn register_shortcuts(app: &tauri::App) {
    let app_handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        global_shortcut::register_default_shortcuts(&app_handle).await;
    });
}

/// 启动显示器热插拔监听（M20）。
///
/// WndProc 回调只向 unbounded channel 投信；异步侧做 300ms debounce 后调用
/// `sync_monitors_from_system()` 完成 DB → slot → reconcile 闭环。
/// 非 Windows 平台无 Win32 消息源，直接返回。
#[cfg(target_os = "windows")]
fn start_monitor_hotplug(app: &tauri::App) {
    use std::time::Duration;

    let app_handle = app.handle().clone();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    if !crate::platform::windows::monitor_hotplug::start(&app_handle, tx) {
        return;
    }

    // 主窗口销毁前还原 WndProc：必须在 HWND 仍有效时调用，
    // 否则 HWND 被系统回收复用后残留表项会引发野指针跳转。
    if let Some(window) = app_handle.get_webview_window("main") {
        window.on_window_event(|event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                crate::platform::windows::monitor_hotplug::stop();
            }
        });
    }

    tauri::async_runtime::spawn(async move {
        // 首条消息立即消费；后续 300ms 内的连发合并为一次。
        while rx.recv().await.is_some() {
            loop {
                match tokio::time::timeout(Duration::from_millis(300), rx.recv()).await {
                    Ok(Some(_)) => continue, // 合并连发
                    Ok(None) => return,      // channel 关闭
                    Err(_) => break,         // 静默期到，执行同步
                }
            }
            let app_ctx = app_handle.state::<AppContext>();
            let mut manager = app_ctx.unified_manager.lock().await;
            if let Err(e) = manager.sync_monitors_from_system().await {
                log::warn!("[MonitorHotplug] 显示器同步失败: {:#}", e);
            }
        }
    });
}

#[cfg(not(target_os = "windows"))]
fn start_monitor_hotplug(_app: &tauri::App) {}

/// 启动后扫一次过期回收站，不阻塞启动。失败只记日志。
fn spawn_trash_purge(app: &tauri::App) {
    let app_handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        crate::services::trash_maintenance::purge_expired_on_startup(&app_handle).await;
    });
}
