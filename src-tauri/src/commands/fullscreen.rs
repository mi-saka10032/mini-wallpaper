use std::sync::Arc;

use tauri::State;
use tokio::sync::Mutex;

use crate::ctx::AppContext;
use crate::runtime::fullscreen_detector::{FullscreenDetectionTask, FULLSCREEN_TIMER_KEY};
use crate::runtime::Scheduler;
use crate::services::app_setting_service;

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
    ctx: State<'_, AppContext>,
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
) -> Result<(), String> {
    let should_start = app_setting_service::get(&ctx.db, "pause_on_fullscreen")
        .await
        .unwrap_or(None)
        .map(|v| v == "true")
        .unwrap_or(false);

    if should_start {
        let mut sched = scheduler.lock().await;
        if !sched.is_running(FULLSCREEN_TIMER_KEY) {
            sched.spawn(
                FULLSCREEN_TIMER_KEY.to_string(),
                FullscreenDetectionTask { app: ctx.app_handle.clone() },
            );
        }
    }

    Ok(())
}