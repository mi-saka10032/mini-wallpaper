use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // SQLite 支持 ALTER TABLE ADD COLUMN（无外键约束时可直接加）
        db.execute_unprepared(
            "ALTER TABLE monitor_configs ADD COLUMN active INTEGER NOT NULL DEFAULT 0",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // SQLite 不直接支持 DROP COLUMN（3.35.0+ 才支持），
        // 用重建表方式回退
        db.execute_unprepared(
            "CREATE TABLE monitor_configs_backup (
                id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
                monitor_id TEXT NOT NULL UNIQUE,
                display_mode TEXT NOT NULL DEFAULT 'independent',
                wallpaper_id INTEGER,
                collection_id INTEGER,
                fit_mode TEXT NOT NULL DEFAULT 'cover',
                play_mode TEXT NOT NULL DEFAULT 'sequential',
                play_interval INTEGER NOT NULL DEFAULT 300,
                is_enabled INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (wallpaper_id) REFERENCES wallpapers(id) ON DELETE SET NULL,
                FOREIGN KEY (collection_id) REFERENCES collections(id) ON DELETE SET NULL
            )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO monitor_configs_backup
                (id, monitor_id, display_mode, wallpaper_id, collection_id, fit_mode, play_mode, play_interval, is_enabled, updated_at)
             SELECT id, monitor_id, display_mode, wallpaper_id, collection_id, fit_mode, play_mode, play_interval, is_enabled, updated_at
             FROM monitor_configs",
        )
        .await?;

        db.execute_unprepared("DROP TABLE monitor_configs").await?;
        db.execute_unprepared("ALTER TABLE monitor_configs_backup RENAME TO monitor_configs")
            .await?;

        Ok(())
    }
}
