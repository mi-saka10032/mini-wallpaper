use sea_orm::DatabaseConnection;
use tauri::State;

use crate::entities::monitor_config;
use crate::services::monitor_config_service;
use crate::services::timer_manager::{
    self, carousel_key, collection_has_enough_wallpapers, should_start_timer,
};
use crate::utils::timer_registry::TimerRegistryState;

use log::info;

/// 获取所有显示器配置
#[tauri::command]
pub async fn get_monitor_configs(
    db: State<'_, DatabaseConnection>,
) -> Result<Vec<monitor_config::Model>, String> {
    monitor_config_service::get_all(db.inner())
        .await
        .map_err(|e| e.to_string())
}

/// 根据 monitor_id 获取配置
#[tauri::command]
pub async fn get_monitor_config(
    db: State<'_, DatabaseConnection>,
    monitor_id: String,
) -> Result<Option<monitor_config::Model>, String> {
    monitor_config_service::get_by_monitor_id(db.inner(), &monitor_id)
        .await
        .map_err(|e| e.to_string())
}

/// 创建或更新显示器配置
///
/// upsert 后自动管理定时器：
/// - 满足轮播条件 → 通过 TimerRegistry 启动/重启定时器
/// - 不满足 → 通过 TimerRegistry 停止定时器
#[tauri::command]
pub async fn upsert_monitor_config(
    db: State<'_, DatabaseConnection>,
    registry: State<'_, TimerRegistryState>,
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
        db.inner(),
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
    manage_timer_for_config(db.inner(), &registry, &app_handle, &config).await;

    Ok(config)
}

/// 根据 config 状态管理定时器
async fn manage_timer_for_config(
    db: &sea_orm::DatabaseConnection,
    registry: &TimerRegistryState,
    app_handle: &tauri::AppHandle,
    config: &monitor_config::Model,
) {
    let key = carousel_key(&config.monitor_id);
    let mut reg = registry.lock().await;

    if should_start_timer(config) {
        let cid = config.collection_id.unwrap(); // safe: should_start_timer 已检查

        // 检查收藏夹壁纸数 > 1
        match collection_has_enough_wallpapers(db, cid).await {
            Ok(true) => {
                let handle = timer_manager::spawn_carousel_task(
                    config.monitor_id.clone(),
                    db.clone(),
                    app_handle.clone(),
                );
                reg.restart(key, handle);
            }
            Ok(false) => {
                reg.stop(&key);
            }
            Err(e) => {
                log::warn!(
                    "[upsert] Failed to check collection wallpapers: {}",
                    e
                );
                reg.stop(&key);
            }
        }
    } else {
        reg.stop(&key);
    }
}

/// 删除显示器配置
#[tauri::command]
pub async fn delete_monitor_config(
    db: State<'_, DatabaseConnection>,
    registry: State<'_, TimerRegistryState>,
    id: i32,
    monitor_id: Option<String>,
) -> Result<(), String> {
    // 先停止可能存在的定时器
    if let Some(mid) = &monitor_id {
        let mut reg = registry.lock().await;
        reg.stop(&carousel_key(mid));
    }

    monitor_config_service::delete(db.inner(), id)
        .await
        .map_err(|e| e.to_string())
}

/// 启动所有满足轮播条件的定时器（应用启动时由前端调用）
#[tauri::command]
pub async fn start_timers(
    db: State<'_, DatabaseConnection>,
    registry: State<'_, TimerRegistryState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let configs = monitor_config_service::get_all(db.inner())
        .await
        .map_err(|e| e.to_string())?;

    let mut reg = registry.lock().await;

    for config in &configs {
        if should_start_timer(config) {
            let cid = config.collection_id.unwrap();
            match collection_has_enough_wallpapers(db.inner(), cid).await {
                Ok(true) => {
                    let key = carousel_key(&config.monitor_id);
                    let handle = timer_manager::spawn_carousel_task(
                        config.monitor_id.clone(),
                        db.inner().clone(),
                        app_handle.clone(),
                    );
                    reg.register(key, handle);
                    info!("[start_timers] 启动定时器: {}", config.monitor_id);
                }
                Ok(false) => {}
                Err(e) => {
                    log::warn!("[start_timers] 检查收藏夹失败 {}: {}", config.monitor_id, e);
                }
            }
        }
    }

    Ok(())
}
