use std::collections::HashMap;
use std::sync::Arc;

use log::info;
use tauri::State;
use tokio::sync::Mutex;

use crate::ctx::AppContext;
use crate::runtime::fullscreen_detector::{FullscreenDetectionTask, FULLSCREEN_TIMER_KEY};
use crate::runtime::Scheduler;
use crate::dto::app_setting_dto::{GetSettingRequest, SetSettingRequest};
use crate::dto::Validated;
use crate::services::app_setting_service;

/// 已知的 setting key 常量（与前端 SETTING_KEYS 保持一致）
pub(crate) mod keys {
    pub const THEME: &str = "theme";
    pub const LANGUAGE: &str = "language";
    pub const CLOSE_TO_TRAY: &str = "close_to_tray";
    pub const PAUSE_ON_FULLSCREEN: &str = "pause_on_fullscreen";
    pub const GLOBAL_VOLUME: &str = "global_volume";
    pub const SHORTCUT_NEXT_WALLPAPER: &str = "shortcut_next_wallpaper";
    pub const SHORTCUT_PREV_WALLPAPER: &str = "shortcut_prev_wallpaper";
}

/// 获取所有设置（返回 key-value 对象）
#[tauri::command]
pub async fn get_settings(
    ctx: State<'_, AppContext>,
) -> Result<HashMap<String, String>, String> {
    let settings = app_setting_service::get_all(&ctx.db)
        .await
        .map_err(|e| e.to_string())?;

    let map: HashMap<String, String> = settings
        .into_iter()
        .map(|s| (s.key, s.value))
        .collect();

    Ok(map)
}

/// 获取单个设置值
#[tauri::command]
pub async fn get_setting(
    ctx: State<'_, AppContext>,
    req: Validated<GetSettingRequest>,
) -> Result<Option<String>, String> {
    let req = req.into_inner();
    app_setting_service::get(&ctx.db, &req.key)
        .await
        .map_err(|e| e.to_string())
}

/// 设置键值对（写入 DB + 按 key 触发副作用）
///
/// 统一入口：前端所有 setting 变更都通过此 command，
/// 内部通过 match key 模式，在写入 DB 后立即执行对应的副作用，
/// 确保设置变更即时生效。
#[tauri::command]
pub async fn set_setting(
    ctx: State<'_, AppContext>,
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
    req: Validated<SetSettingRequest>,
) -> Result<(), String> {
    let req = req.into_inner();

    // 跨字段校验：按 key 校验 value 格式
    req.validate_value_format()?;

    // 1. 写入 DB
    app_setting_service::set(&ctx.db, &req.key, &req.value)
        .await
        .map_err(|e| e.to_string())?;

    // 2. 按 key 执行副作用
    apply_setting_side_effect(&req.key, &req.value, &ctx, &scheduler).await;

    Ok(())
}

/// 音量变更事件 payload（广播给所有壁纸窗口）
#[derive(Clone, serde::Serialize)]
struct VolumeChangedPayload {
    /// 音量值 0-100
    volume: u32,
}

/// 根据 setting key 执行对应的副作用
///
/// 每个需要"写入后立即生效"的 key 在此注册处理逻辑。
/// 无副作用的 key（如 theme、language）仅写入 DB，前端自行响应。
async fn apply_setting_side_effect(
    key: &str,
    value: &str,
    ctx: &AppContext,
    scheduler: &Arc<Mutex<Scheduler>>,
) {
    match key {
        keys::PAUSE_ON_FULLSCREEN => {
            let enabled = value == "true";
            let mut sched = scheduler.lock().await;
            if enabled {
                if !sched.is_running(FULLSCREEN_TIMER_KEY) {
                    sched.spawn(
                        FULLSCREEN_TIMER_KEY.to_string(),
                        FullscreenDetectionTask { app: ctx.app_handle.clone() },
                    );
                    info!("[Setting] 全屏检测已启动");
                }
            } else {
                sched.stop(FULLSCREEN_TIMER_KEY);
                info!("[Setting] 全屏检测已停止");
            }
        }
        keys::GLOBAL_VOLUME => {
            // DTO 层已校验 value 为 0~100 的合法整数，此处 parse 安全
            let volume = value.parse::<u32>().unwrap_or(0).min(100);
            let mgr = ctx.window_manager.lock().await;
            mgr.broadcast("volume-changed", &VolumeChangedPayload { volume });
            info!("[Setting] 音量已更新: {}%", volume);
        }
        _ => {
            // 无副作用的 key，仅写入 DB 即可
        }
    }
}