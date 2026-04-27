use sea_orm::DatabaseConnection;
use tauri::State;

use crate::platform::fullscreen_detector::{self, FULLSCREEN_TIMER_KEY};
use crate::services::app_setting_service;
use crate::utils::timer_registry::TimerRegistryState;

/// 初始化全屏检测（由前端 App.tsx useEffect 首次且唯一一次调用）
///
/// 读取 DB 中 pause_on_fullscreen 设置，若为 true 则注册检测任务。
/// 将启动时机从 setup 异步延迟改为前端初始化完成后主动 invoke，
/// 更加优雅且符合前端驱动的交互模式。
///
/// 后续的启停由 `set_setting(key="pause_on_fullscreen")` 的副作用统一管理，
/// 不再需要单独的 set_fullscreen_detection command。
#[tauri::command]
pub async fn init_fullscreen_detection(
    db: State<'_, DatabaseConnection>,
    registry: State<'_, TimerRegistryState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let should_start = app_setting_service::get(db.inner(), "pause_on_fullscreen")
        .await
        .unwrap_or(None)
        .map(|v| v == "true")
        .unwrap_or(false);

    if should_start {
        let mut reg = registry.lock().await;
        if !reg.is_running(FULLSCREEN_TIMER_KEY) {
            let handle = fullscreen_detector::spawn_detection_task(app_handle);
            reg.register(FULLSCREEN_TIMER_KEY.to_string(), handle);
        }
    }

    Ok(())
}
