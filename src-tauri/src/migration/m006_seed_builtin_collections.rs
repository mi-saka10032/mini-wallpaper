//! 内置「我喜欢」收藏夹种子数据 + `is_builtin` 列引入
//!
//! ## 背景
//!
//! - 项目长期收藏语义由 `collections` + `collection_wallpapers` 多对多承载，
//!   `wallpapers.is_favorite` 字段从未被业务代码使用，是早期遗留的"僵尸字段"。
//! - 全局快捷键 `ToggleFavorite` 与未来的"红心一键收藏"按钮需要一个**全局唯一**
//!   的目标收藏夹，因此本次迁移引入"系统内置收藏夹"概念：
//!   - 表里多一列 `is_builtin INTEGER NOT NULL DEFAULT 0`
//!   - 全表至多一行 `is_builtin = 1`，名为「我喜欢」
//!   - 业务层禁止重命名 / 删除该行
//!
//! ## 设计要点
//!
//! 1. **不强行指定 id**：让数据库自增分配。老用户的 `collections.id=1` 大概率
//!    已被既有收藏夹占用，强行 INSERT id=1 会导致主键冲突或覆盖用户数据。
//!    业务代码定位"我喜欢"时统一用 `WHERE is_builtin = 1`。
//! 2. **幂等 + 防御性插入**：`INSERT ... WHERE NOT EXISTS` 保证即使迁移被
//!    人为重跑也不会插入第二条 builtin。
//! 3. **数据库层兜底**：再加一条 partial unique index，从 schema 层保证全表
//!    至多一行 builtin，业务代码若失误也无法绕过。
//! 4. **sort_order = -1**：让内置收藏夹永远排在用户自建收藏夹之前
//!    （`get_all` 按 sort_order ASC 排序）。
//!
//! ## 兼容性
//!
//! - 新装应用：m001~m005 建好空表后，本迁移在空表上插入第一条记录。
//! - 老用户升级：`ADD COLUMN ... DEFAULT 0` 给所有既有行打上 `is_builtin=0`，
//!   不影响任何现有收藏夹；新「我喜欢」由数据库自增分配 id，与既有 id 不冲突。

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 1. 给 collections 表新增 is_builtin 列（默认 0，老数据自动打标）
        manager
            .alter_table(
                Table::alter()
                    .table(Collections::Table)
                    .add_column(
                        ColumnDef::new(Collections::IsBuiltin)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .to_owned(),
            )
            .await?;

        // 2. 数据库层兜底：partial unique index，全表至多一条 is_builtin=1
        //
        // SeaQuery 的 Index DSL 不支持 partial index `WHERE` 子句，
        // 这里使用原生 SQL（SQLite 自 3.8.0 起支持，所有当前可用的 Tauri 平台均满足）。
        let backend = manager.get_database_backend();
        manager
            .get_connection()
            .execute(sea_orm::Statement::from_string(
                backend,
                "CREATE UNIQUE INDEX IF NOT EXISTS \
                 idx_collections_unique_builtin \
                 ON collections (is_builtin) WHERE is_builtin = 1"
                    .to_string(),
            ))
            .await?;

        // 3. 条件插入「我喜欢」内置收藏夹
        //
        // - 不指定 id，让数据库自增分配（老用户的 id=1 通常已被占用）
        // - WHERE NOT EXISTS 保证幂等：即使迁移被人为重跑也只插一次
        // - sort_order = -1 让内置收藏夹永远排在最前面
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let sql = format!(
            "INSERT INTO collections (name, sort_order, created_at, updated_at, is_builtin) \
             SELECT '我喜欢', -1, '{now}', '{now}', 1 \
             WHERE NOT EXISTS (SELECT 1 FROM collections WHERE is_builtin = 1)"
        );
        manager
            .get_connection()
            .execute(sea_orm::Statement::from_string(backend, sql))
            .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // 生产环境不支持回滚，如需变更请新建 migration
        Ok(())
    }
}

/// 与 m002 中保持同名同变体，仅 IsBuiltin 是新增；
/// 这里独立声明一份是因为 m002 的 `Collections` enum 是 `pub(super)` 不可见，
/// 且各 migration 文件按惯例声明各自需要的 Iden 集合。
#[derive(DeriveIden)]
enum Collections {
    Table,
    IsBuiltin,
}
