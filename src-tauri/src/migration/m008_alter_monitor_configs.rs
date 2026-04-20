use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // =================================================================
        // SQLite 不支持对带外键约束的列执行 ALTER TABLE DROP COLUMN，
        // 因此采用经典的重建表方式：
        //   1. 创建新表（不含 playlist_id，新增 collection_id 等列）
        //   2. 迁移旧数据
        //   3. 删除旧表
        //   4. 重命名新表
        // =================================================================

        // 1. 创建新表
        db.execute_unprepared(
            "CREATE TABLE monitor_configs_new (
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
            )"
        ).await?;

        // 2. 迁移旧数据（只复制新表中存在的列，playlist_id 丢弃）
        db.execute_unprepared(
            "INSERT INTO monitor_configs_new (id, monitor_id, display_mode, wallpaper_id, fit_mode, updated_at)
             SELECT id, monitor_id, display_mode, wallpaper_id, fit_mode, updated_at
             FROM monitor_configs"
        ).await?;

        // 3. 删除旧表
        db.execute_unprepared("DROP TABLE monitor_configs").await?;

        // 4. 重命名新表
        db.execute_unprepared("ALTER TABLE monitor_configs_new RENAME TO monitor_configs").await?;

        // =================================================================
        // 删除废弃的 playlist 相关表
        // =================================================================
        db.execute_unprepared("DROP TABLE IF EXISTS playlist_wallpapers").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS playlists").await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // 恢复 playlists 表
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS playlists (
                id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                description TEXT,
                play_mode TEXT NOT NULL DEFAULT 'sequential',
                play_interval INTEGER NOT NULL DEFAULT 300,
                is_active INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )"
        ).await?;

        // 恢复 playlist_wallpapers 表
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS playlist_wallpapers (
                id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
                playlist_id INTEGER NOT NULL,
                wallpaper_id INTEGER NOT NULL,
                sort_order INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (playlist_id) REFERENCES playlists(id) ON DELETE CASCADE,
                FOREIGN KEY (wallpaper_id) REFERENCES wallpapers(id) ON DELETE CASCADE
            )"
        ).await?;

        // 重建 monitor_configs 恢复 playlist_id
        db.execute_unprepared(
            "CREATE TABLE monitor_configs_old (
                id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
                monitor_id TEXT NOT NULL UNIQUE,
                display_mode TEXT NOT NULL DEFAULT 'independent',
                wallpaper_id INTEGER,
                playlist_id INTEGER,
                fit_mode TEXT NOT NULL DEFAULT 'cover',
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (wallpaper_id) REFERENCES wallpapers(id) ON DELETE SET NULL,
                FOREIGN KEY (playlist_id) REFERENCES playlists(id) ON DELETE SET NULL
            )"
        ).await?;

        db.execute_unprepared(
            "INSERT INTO monitor_configs_old (id, monitor_id, display_mode, wallpaper_id, fit_mode, updated_at)
             SELECT id, monitor_id, display_mode, wallpaper_id, fit_mode, updated_at
             FROM monitor_configs"
        ).await?;

        db.execute_unprepared("DROP TABLE monitor_configs").await?;
        db.execute_unprepared("ALTER TABLE monitor_configs_old RENAME TO monitor_configs").await?;

        Ok(())
    }
}
