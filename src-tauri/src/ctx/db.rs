use anyhow::Result;
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;
use std::path::PathBuf;
use tauri::Manager;
use log::info;

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

    // 通过环境变量 SQL_LOG=1 开启 SQL 语句日志，默认关闭
    let sql_logging = std::env::var("SQL_LOG")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let mut opt = ConnectOptions::new(&db_url);
    opt.sqlx_logging(sql_logging);
    if sql_logging {
        opt.sqlx_logging_level(log::LevelFilter::Debug);
    }

    let db = Database::connect(opt).await?;
    let connect_elapsed = start.elapsed();
    info!("[DB] Connected in {:.0?}", connect_elapsed);

    Migrator::up(&db, None).await?;
    let migrate_elapsed = start.elapsed();
    info!("[DB] Migrations completed in {:.0?}", migrate_elapsed);

    info!(
        "[DB] SQLite initialized at: {} (total: {:.0?}), sql_logging={}",
        db_path.display(),
        start.elapsed(),
        sql_logging
    );
    Ok(db)
}