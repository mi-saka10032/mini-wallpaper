use std::sync::Arc;

use tauri::{Manager, State};
use tokio::sync::Mutex;

use crate::ctx::AppContext;
use crate::dto::backup_dto::{ExportBackupRequest, ImportBackupRequest};
use crate::dto::Validated;
use crate::events::{BackupProgressPayload, TypedEmit};
use crate::runtime::Scheduler;
use crate::services::backup_service;
use crate::utils::progress_io::ByteProgressFn;

use super::error::CommandResult;

/// 导出备份到指定路径
#[tauri::command]
pub async fn export_backup(
    ctx: State<'_, AppContext>,
    req: Validated<ExportBackupRequest>,
) -> CommandResult<String> {
    let _guard = ctx
        .backup_lock
        .try_lock()
        .map_err(|_| "已有备份任务正在执行，请等待完成后再试".to_string())?;

    let req = req.into_inner();
    let app_data_dir = ctx.app_handle.path().app_data_dir()?;
    let snapshot_dir =
        std::env::temp_dir().join(format!("mini-wallpaper-backup-{}", std::process::id()));
    let snapshot = snapshot_dir.join("app.db");
    if let Err(error) = backup_service::snapshot_database(&ctx.db, &snapshot).await {
        let _ = std::fs::remove_dir_all(&snapshot_dir);
        return Err(error.to_string().into());
    }

    let handle = ctx.app_handle.clone();
    let progress_cb: Option<ByteProgressFn> = Some(Box::new(move |current, total| {
        let _ = handle.typed_emit(&BackupProgressPayload { current, total });
    }));

    // 在 blocking 线程中执行 IO 密集操作。压缩包里的 app.db 是上面的快照，
    // 不读取正在使用的数据库文件和它的 WAL。
    let out = req.output_path.clone();
    let export_result = tauri::async_runtime::spawn_blocking(move || {
        backup_service::export_backup(
            &app_data_dir,
            &snapshot,
            std::path::Path::new(&out),
            progress_cb,
        )
    })
    .await;
    let _ = std::fs::remove_dir_all(&snapshot_dir);
    export_result
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    Ok(req.output_path)
}

/// 从 zip 文件导入备份
#[tauri::command]
pub async fn import_backup(
    ctx: State<'_, AppContext>,
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
    req: Validated<ImportBackupRequest>,
) -> CommandResult<u64> {
    let _guard = ctx
        .backup_lock
        .try_lock()
        .map_err(|_| "已有备份任务正在执行，请等待完成后再试".to_string())?;

    let req = req.into_inner();
    let app_data_dir = ctx.app_handle.path().app_data_dir()?;
    // 临时目录放在应用目录内，后续 rename 与数据库在同一卷上。
    let staging = app_data_dir.join(".backup-import-staging");
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;

    let handle = ctx.app_handle.clone();
    let progress_cb: Option<ByteProgressFn> = Some(Box::new(move |current, total| {
        let _ = handle.typed_emit(&BackupProgressPayload { current, total });
    }));

    let zip = req.zip_path.clone();
    let staging_for_extract = staging.clone();
    let extracted = tauri::async_runtime::spawn_blocking(move || {
        backup_service::import_backup(
            &staging_for_extract,
            std::path::Path::new(&zip),
            progress_cb,
        )
    })
    .await
    .map_err(|e| e.to_string());
    let count = match extracted {
        Ok(Ok(count)) => count,
        Ok(Err(error)) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(error.to_string().into());
        }
        Err(error) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(error.into());
        }
    };

    // 先停掉会写库的后台任务，再关闭共享连接，最后才替换数据库文件。
    // 连接关闭后本次进程不再打开新库，界面已要求导入后重启。
    let scheduler_handle: Arc<Mutex<Scheduler>> = scheduler.inner().clone();
    scheduler_handle.lock().await.stop_all();
    if let Err(error) = ctx.db.close_by_ref().await {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error.to_string().into());
    }

    let staging_for_install = staging.clone();
    let app_data_for_install = app_data_dir.clone();
    let installed = tauri::async_runtime::spawn_blocking(move || {
        backup_service::install_staged_backup(&staging_for_install, &app_data_for_install)
    })
    .await;
    let _ = std::fs::remove_dir_all(&staging);
    installed
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    Ok(count)
}

/// 获取应用数据总大小（字节）
#[tauri::command]
pub async fn get_data_size(ctx: State<'_, AppContext>) -> CommandResult<u64> {
    let app_data_dir = ctx.app_handle.path().app_data_dir()?;
    Ok(backup_service::get_data_size(&app_data_dir))
}
