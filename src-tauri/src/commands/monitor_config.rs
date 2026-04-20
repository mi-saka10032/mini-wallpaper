use tauri::State;

use crate::entities::monitor_config;
use crate::services::monitor_config_service;
use crate::services::timer_manager::{
    collection_has_enough_wallpapers, should_start_timer, TimerManagerState,
};
use crate::AppState;

/// 获取所有显示器配置
#[tauri::command]
pub async fn get_monitor_configs(
    state: State<'_, AppState>,
) -> Result<Vec<monitor_config::Model>, String> {
    monitor_config_service::get_all(&state.db)
        .await
        .map_err(|e| e.to_string())
}

/// 根据 monitor_id 获取配置
#[tauri::command]
pub async fn get_monitor_config(
    state: State<'_, AppState>,
    monitor_id: String,
) -> Result<Option<monitor_config::Model>, String> {
    monitor_config_service::get_by_monitor_id(&state.db, &monitor_id)
        .await
        .map_err(|e| e.to_string())
}

/// 创建或更新显示器配置
///
/// upsert 后自动管理定时器：
/// - 满足轮播条件 → 启动/重启定时器
/// - 不满足 → 停止定时器
#[tauri::command]
pub async fn upsert_monitor_config(
    state: State<'_, AppState>,
    timer_state: State<'_, TimerManagerState>,
    app_handle: tauri::AppHandle,
    monitor_id: String,
    wallpaper_id: Option<i32>,
    collection_id: Option<i32>,
    clear_collection: Option<bool>,
    display_mode: Option<String>,
    fit_mode: Option<String>,
    play_mode: Option<String>,
    play_interval: Option<i32>,
    is_enabled: Option<bool>,
    active: Option<bool>,
) -> Result<monitor_config::Model, String> {
    let config = monitor_config_service::upsert(
        &state.db,
        &monitor_id,
        wallpaper_id,
        collection_id,
        clear_collection,
        display_mode.as_deref(),
        fit_mode.as_deref(),
        play_mode.as_deref(),
        play_interval,
        is_enabled,
        active,
    )
    .await
    .map_err(|e| e.to_string())?;

    // ===== 定时器管理 =====
    manage_timer_for_config(&state.db, &timer_state, &app_handle, &config).await;

    Ok(config)
}

/// 根据 config 状态管理定时器
async fn manage_timer_for_config(
    db: &sea_orm::DatabaseConnection,
    timer_state: &TimerManagerState,
    app_handle: &tauri::AppHandle,
    config: &monitor_config::Model,
) {
    let mut manager = timer_state.lock().await;

    if should_start_timer(config) {
        let cid = config.collection_id.unwrap(); // safe: should_start_timer 已检查

        // 检查收藏夹壁纸数 > 1
        match collection_has_enough_wallpapers(db, cid).await {
            Ok(true) => {
                manager.restart(
                    config.monitor_id.clone(),
                    db.clone(),
                    app_handle.clone(),
                );
            }
            Ok(false) => {
                manager.stop(&config.monitor_id);
            }
            Err(e) => {
                eprintln!(
                    "[upsert] Failed to check collection wallpapers: {}",
                    e
                );
                manager.stop(&config.monitor_id);
            }
        }
    } else {
        manager.stop(&config.monitor_id);
    }
}

/// 删除显示器配置
#[tauri::command]
pub async fn delete_monitor_config(
    state: State<'_, AppState>,
    timer_state: State<'_, TimerManagerState>,
    id: i32,
    monitor_id: Option<String>,
) -> Result<(), String> {
    // 先停止可能存在的定时器
    if let Some(mid) = &monitor_id {
        let mut manager = timer_state.lock().await;
        manager.stop(mid);
    }

    monitor_config_service::delete(&state.db, id)
        .await
        .map_err(|e| e.to_string())
}
