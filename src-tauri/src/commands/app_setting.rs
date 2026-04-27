use std::collections::HashMap;

use log::info;
use sea_orm::DatabaseConnection;
use tauri::State;

use crate::platform::fullscreen_detector::{self, FULLSCREEN_TIMER_KEY};
use crate::services::app_setting_service;
use crate::utils::timer_registry::TimerRegistryState;

/// 已知的 setting key 常量（与前端 SETTING_KEYS 保持一致）
mod keys {
    pub const PAUSE_ON_FULLSCREEN: &str = "pause_on_fullscreen";
    // 后续可扩展更多需要副作用的 key：
    // pub const GLOBAL_VOLUME: &str = "global_volume";
    // pub const CLOSE_TO_TRAY: &str = "close_to_tray";
}

/// 获取所有设置（返回 key-value 对象）
#[tauri::command]
pub async fn get_settings(
    db: State<'_, DatabaseConnection>,
) -> Result<HashMap<String, String>, String> {
    let settings = app_setting_service::get_all(db.inner())
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
    db: State<'_, DatabaseConnection>,
    key: String,
) -> Result<Option<String>, String> {
    app_setting_service::get(db.inner(), &key)
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
    db: State<'_, DatabaseConnection>,
    registry: State<'_, TimerRegistryState>,
    app_handle: tauri::AppHandle,
    key: String,
    value: String,
) -> Result<(), String> {
    // 1. 写入 DB
    app_setting_service::set(db.inner(), &key, &value)
        .await
        .map_err(|e| e.to_string())?;

    // 2. 按 key 执行副作用
    apply_setting_side_effect(&key, &value, &registry, &app_handle).await;

    Ok(())
}

/// 根据 setting key 执行对应的副作用
///
/// 每个需要"写入后立即生效"的 key 在此注册处理逻辑。
/// 无副作用的 key（如 theme、language）仅写入 DB，前端自行响应。
async fn apply_setting_side_effect(
    key: &str,
    value: &str,
    registry: &TimerRegistryState,
    app_handle: &tauri::AppHandle,
) {
    match key {
        keys::PAUSE_ON_FULLSCREEN => {
            let enabled = value == "true";
            let mut reg = registry.lock().await;
            if enabled {
                if !reg.is_running(FULLSCREEN_TIMER_KEY) {
                    let handle =
                        fullscreen_detector::spawn_detection_task(app_handle.clone());
                    reg.register(FULLSCREEN_TIMER_KEY.to_string(), handle);
                    info!("[Setting] 全屏检测已启动");
                }
            } else {
                reg.stop(FULLSCREEN_TIMER_KEY);
                info!("[Setting] 全屏检测已停止");
            }
        }
        // 后续扩展示例：
        // keys::GLOBAL_VOLUME => { /* 通知壁纸窗口调整音量 */ }
        // keys::CLOSE_TO_TRAY => { /* 更新托盘行为 */ }
        _ => {
            // 无副作用的 key，仅写入 DB 即可
        }
    }
}
