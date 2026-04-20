use std::collections::HashMap;

use tauri::State;

use crate::services::app_setting_service;
use crate::AppState;

/// 获取所有设置（返回 key-value 对象）
#[tauri::command]
pub async fn get_settings(
    state: State<'_, AppState>,
) -> Result<HashMap<String, String>, String> {
    let settings = app_setting_service::get_all(&state.db)
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
    state: State<'_, AppState>,
    key: String,
) -> Result<Option<String>, String> {
    app_setting_service::get(&state.db, &key)
        .await
        .map_err(|e| e.to_string())
}

/// 设置键值对
#[tauri::command]
pub async fn set_setting(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> Result<(), String> {
    app_setting_service::set(&state.db, &key, &value)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
