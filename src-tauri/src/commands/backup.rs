use tauri::{Emitter, Manager};

use crate::services::backup_service;

/// 进度事件 payload
#[derive(Clone, serde::Serialize)]
struct BackupProgress {
    current: u64,
    total: u64,
}

/// 导出备份到指定路径
#[tauri::command]
pub async fn export_backup(
    output_path: String,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;

    let handle = app_handle.clone();
    let progress_cb: Option<backup_service::ProgressFn> = Some(Box::new(move |current, total| {
        let _ = handle.emit("backup-progress", BackupProgress { current, total });
    }));

    // 在 blocking 线程中执行 IO 密集操作
    let out = output_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        backup_service::export_backup(
            &app_data_dir,
            &std::path::Path::new(&out),
            progress_cb,
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    Ok(output_path)
}

/// 从 zip 文件导入备份
#[tauri::command]
pub async fn import_backup(
    zip_path: String,
    app_handle: tauri::AppHandle,
) -> Result<u64, String> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;

    let handle = app_handle.clone();
    let progress_cb: Option<backup_service::ProgressFn> = Some(Box::new(move |current, total| {
        let _ = handle.emit("backup-progress", BackupProgress { current, total });
    }));

    let zip = zip_path.clone();
    let count = tauri::async_runtime::spawn_blocking(move || {
        backup_service::import_backup(
            &app_data_dir,
            &std::path::Path::new(&zip),
            progress_cb,
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    Ok(count)
}

/// 获取应用数据总大小（字节）
#[tauri::command]
pub async fn get_data_size(app_handle: tauri::AppHandle) -> Result<u64, String> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;

    Ok(backup_service::get_data_size(&app_data_dir))
}
