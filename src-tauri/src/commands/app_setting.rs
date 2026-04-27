use std::collections::HashMap;

use sea_orm::DatabaseConnection;
use tauri::State;

use crate::services::app_setting_service;

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

/// 设置键值对
#[tauri::command]
pub async fn set_setting(
    db: State<'_, DatabaseConnection>,
    key: String,
    value: String,
) -> Result<(), String> {
    app_setting_service::set(db.inner(), &key, &value)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
