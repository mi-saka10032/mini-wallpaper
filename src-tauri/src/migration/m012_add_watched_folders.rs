//! 目录监听：`watched_folders` 表 + `wallpapers` 来源标记。
//!
//! ## 设计要点
//!
//! - `path` 是 unique key：一个目录只允许一条监听记录。目录失效后记录保留
//!   （弹窗提示），用户用 opener 重选同一路径即复用该行重新激活。
//! - `collection_id` 可为 NULL：失效时收藏夹被删除，关联一并清空。
//! - `is_external` 用整数标志而非字符串：0 = 本地导入（AppData 内有副本），
//!   1 = 外部监听（源文件不被复制）。`DEFAULT 0` 让历史行自动归为本地，
//!   无需任何数据回填脚本。
//! - 隔离查询不在业务代码里散落加条件，而是收敛到 `wallpaper_service::local_only()`。
//!
//! ## 索引
//!
//! 实测（5000 行）显示给低基数列 `is_external` 单独建索引会让隔离查询慢约
//! 45%，因此这里**不建**。只建三个真正服务于按目录查找的索引；其中
//! `(watched_folder_id, source_key)` 用部分唯一索引，只索引外部监听行，
//! 避免本地行参与、也减小写放大。
//!
//! ## 兼容性
//!
//! - 老用户升级：`ADD COLUMN` 带常量 DEFAULT 是 O(1)，SQLite 不重写表；
//!   其余四列可空，历史行天然为 NULL，语义等价于「非监听壁纸」。
//! - `collections.kind` 是字符串列，新增取值 `watched` 无需改表、无需回填。
//! - `down()` 保持项目约定：生产环境不支持回滚，变更请新建 migration。

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(WatchedFolders::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(WatchedFolders::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(WatchedFolders::Path).string().not_null())
                    .col(ColumnDef::new(WatchedFolders::Name).string().not_null())
                    .col(ColumnDef::new(WatchedFolders::CollectionId).integer())
                    .col(
                        ColumnDef::new(WatchedFolders::Valid)
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .col(ColumnDef::new(WatchedFolders::LastScanAt).string())
                    .col(
                        ColumnDef::new(WatchedFolders::CreatedAt)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(WatchedFolders::UpdatedAt)
                            .string()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // 逐列 ALTER：与 m009/m010 保持一致，避免一次 ALTER 多列在 SQLite 上的差异。
        manager
            .alter_table(
                Table::alter()
                    .table(Wallpapers::Table)
                    .add_column(
                        ColumnDef::new(Wallpapers::IsExternal)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Wallpapers::Table)
                    .add_column(ColumnDef::new(Wallpapers::WatchedFolderId).integer())
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Wallpapers::Table)
                    .add_column(ColumnDef::new(Wallpapers::SourceKey).string())
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Wallpapers::Table)
                    .add_column(ColumnDef::new(Wallpapers::SourceSize).big_integer())
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Wallpapers::Table)
                    .add_column(ColumnDef::new(Wallpapers::SourceMtime).big_integer())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx-watched-folders-path")
                    .table(WatchedFolders::Table)
                    .col(WatchedFolders::Path)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx-watched-folders-collection")
                    .table(WatchedFolders::Table)
                    .col(WatchedFolders::CollectionId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // SQLite 的部分索引无法用 SeaQuery DSL 表达，走原生 SQL（与 m007 同）。
        let backend = manager.get_database_backend();
        manager
            .get_connection()
            .execute(sea_orm::Statement::from_string(
                backend,
                "CREATE UNIQUE INDEX IF NOT EXISTS \"idx-wallpapers-watched-key\" \
                 ON wallpapers (watched_folder_id, source_key) WHERE is_external = 1"
                    .to_string(),
            ))
            .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // 生产环境不支持回滚，如需变更请新建 migration
        Ok(())
    }
}

#[derive(DeriveIden)]
enum WatchedFolders {
    Table,
    Id,
    Path,
    Name,
    CollectionId,
    Valid,
    LastScanAt,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Wallpapers {
    Table,
    IsExternal,
    WatchedFolderId,
    SourceKey,
    SourceSize,
    SourceMtime,
}
