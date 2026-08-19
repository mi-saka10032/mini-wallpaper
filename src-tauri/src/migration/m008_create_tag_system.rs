//! 标签系统数据模型：规范化标签表 + 壁纸-标签多对多
//!
//! ## 背景
//!
//! - 旧 `wallpapers.tags: TEXT` 是彻底的僵尸字段：仅在导入时写死 `NULL`，
//!   无任何业务读取（前端 `config.ts` 有类型声明但同样从未使用）。
//! - 标签系统与智能收藏夹强协同，需要规范化存储以支持：
//!   打标签/取消、标签改名不影响引用、按 id 参与规则查询（防注入）。
//!
//! ## 设计要点
//!
//! - 先 `DROP` 掉未用的 `wallpapers.tags` 列（历史值全为 NULL，无损）。
//! - `tags` 表：`name` 唯一（resolve-or-create 依赖），自增 id。
//! - `wallpaper_tags` join 表：`(wallpaper_id, tag_id)` 复合主键天然去重，
//!   两侧各建索引提升 JOIN/WHERE 性能。
//! - 智能收藏夹规则中标签条件存 **tag id 数组**，查询 join 本表求值。
//!
//! ## 兼容性
//!
//! - 老用户升级：删除既有 `tags` 列，历史值全 NULL，无信息丢失；两张新表 `if_not_exists`。
//! - 新装应用：m001 建表后本迁移紧接删列 + 建表（等价于该列从未存在）。

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 1. 删除僵尸字段 wallpapers.tags（SQLite 3.35.0+ 原生支持 DROP COLUMN）
        let backend = manager.get_database_backend();
        manager
            .get_connection()
            .execute(sea_orm::Statement::from_string(
                backend,
                "ALTER TABLE wallpapers DROP COLUMN tags".to_string(),
            ))
            .await?;

        // 2. 创建 tags 表
        manager
            .create_table(
                Table::create()
                    .table(Tags::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Tags::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Tags::Name).string().not_null())
                    .col(
                        ColumnDef::new(Tags::CreatedAt)
                            .string()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // tags.name 唯一：resolve-or-create 的命中依据，防止重复标签
        manager
            .create_index(
                Index::create()
                    .name("idx-tags-name-unique")
                    .table(Tags::Table)
                    .col(Tags::Name)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // 3. 创建 wallpaper_tags join 表
        manager
            .create_table(
                Table::create()
                    .table(WallpaperTags::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(WallpaperTags::WallpaperId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(WallpaperTags::TagId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(WallpaperTags::CreatedAt)
                            .string()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .primary_key(
                        Index::create()
                            .col(WallpaperTags::WallpaperId)
                            .col(WallpaperTags::TagId),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-wallpaper_tags-wallpaper_id")
                    .table(WallpaperTags::Table)
                    .col(WallpaperTags::WallpaperId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-wallpaper_tags-tag_id")
                    .table(WallpaperTags::Table)
                    .col(WallpaperTags::TagId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // 生产环境不支持回滚，如需变更请新建 migration
        Ok(())
    }
}

#[derive(DeriveIden)]
pub enum Tags {
    Table,
    Id,
    Name,
    CreatedAt,
}

#[derive(DeriveIden)]
pub enum WallpaperTags {
    Table,
    WallpaperId,
    TagId,
    CreatedAt,
}
