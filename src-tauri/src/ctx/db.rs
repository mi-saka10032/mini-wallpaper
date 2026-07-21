use anyhow::Result;
use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection};
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
    // 显式约束连接池为单连接：
    // - 桌面单用户场景，写操作本就被 SQLite 串行化，单连接足够
    // - 保证下列 per-connection PRAGMA（busy_timeout / synchronous）全量生效，
    //   行为完全可预期，不再隐式依赖 sqlx 连接池默认值
    opt.max_connections(1).min_connections(1);
    opt.sqlx_logging(sql_logging);
    if sql_logging {
        opt.sqlx_logging_level(log::LevelFilter::Debug);
    }

    let db = Database::connect(opt).await?;
    let connect_elapsed = start.elapsed();
    info!("[DB] Connected in {:.0?}", connect_elapsed);

    // 显式声明 SQLite 并发/持久化配置，避免隐式依赖驱动默认值：
    // - journal_mode=WAL：读写不互斥，写等待更少（数据库级持久设置）
    // - busy_timeout=5000：拿不到写锁时最多等待 5s 再报 SQLITE_BUSY
    // - synchronous=NORMAL：WAL 下的推荐值，兼顾性能与安全
    //   （表结构无外键约束，删除联动由 service 层手动处理，故不设 foreign_keys）
    for pragma in [
        "PRAGMA journal_mode=WAL;",
        "PRAGMA busy_timeout=5000;",
        "PRAGMA synchronous=NORMAL;",
    ] {
        db.execute_unprepared(pragma)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to apply {}: {}", pragma, e))?;
    }

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