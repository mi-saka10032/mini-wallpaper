use tauri::{Manager, State};

use crate::ctx::AppContext;
use crate::dto::backup_dto::{ExportBackupRequest, ImportBackupRequest};
use crate::dto::Validated;
use crate::events::{BackupProgressPayload, TypedEmit};
use crate::services::backup_service;
use crate::utils::progress_io::ByteProgressFn;

use super::error::CommandResult;

/// 导出备份到指定路径
#[tauri::command]
pub async fn export_backup(
    ctx: State<'_, AppContext>,
    req: Validated<ExportBackupRequest>,
) -> CommandResult<String> {
    let _guard = ctx.backup_lock.try_lock()
        .map_err(|_| "已有备份任务正在执行，请等待完成后再试".to_string())?;

    let req = req.into_inner();
    let app_data_dir = ctx.app_handle.path().app_data_dir()?;

    let handle = ctx.app_handle.clone();
    let progress_cb: Option<ByteProgressFn> = Some(Box::new(move |current, total| {
        let _ = handle.typed_emit(&BackupProgressPayload { current, total });
    }));

    // 在 blocking 线程中执行 IO 密集操作
    let out = req.output_path.clone();
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

    Ok(req.output_path)
}

/// 从 zip 文件导入备份
#[tauri::command]
pub async fn import_backup(
    ctx: State<'_, AppContext>,
    req: Validated<ImportBackupRequest>,
) -> CommandResult<u64> {
    let _guard = ctx.backup_lock.try_lock()
        .map_err(|_| "已有备份任务正在执行，请等待完成后再试".to_string())?;

    let req = req.into_inner();
    let app_data_dir = ctx.app_handle.path().app_data_dir()?;

    let handle = ctx.app_handle.clone();
    let progress_cb: Option<ByteProgressFn> = Some(Box::new(move |current, total| {
        let _ = handle.typed_emit(&BackupProgressPayload { current, total });
    }));

    let zip = req.zip_path.clone();
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
pub async fn get_data_size(
    ctx: State<'_, AppContext>,
) -> CommandResult<u64> {
    let app_data_dir = ctx.app_handle.path().app_data_dir()?;
    Ok(backup_service::get_data_size(&app_data_dir))
}
