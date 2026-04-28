use anyhow::Result;
use sea_orm::{Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;
use std::path::PathBuf;
use tauri::Manager;

use crate::migration::Migrator;

/// 获取数据库文件路径（AppData 目录下）
pub(super) fn get_db_path(app: &tauri::AppHandle) -> Result<PathBuf> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!("Failed to get app data dir: {}", e))?;
    std::fs::create_dir_all(&app_data_dir)?;
    Ok(app_data_dir.join("app.db"))
}

/// 初始化数据库连接并执行迁移
pub(super) async fn init_db(app: &tauri::AppHandle) -> Result<DatabaseConnection> {
    let start = std::time::Instant::now();

    let db_path = get_db_path(app)?;
    let db_url = format!("sqlite:{}?mode=rwc", db_path.display());

    let db = Database::connect(&db_url).await?;
    let connect_elapsed = start.elapsed();
    println!("[DB] Connected in {:.0?}", connect_elapsed);

    Migrator::up(&db, None).await?;
    let migrate_elapsed = start.elapsed();
    println!("[DB] Migrations completed in {:.0?}", migrate_elapsed);

    println!(
        "[DB] SQLite initialized at: {} (total: {:.0?})",
        db_path.display(),
        start.elapsed()
    );
    Ok(db)
}
