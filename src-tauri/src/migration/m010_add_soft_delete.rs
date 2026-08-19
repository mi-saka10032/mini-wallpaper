//! 回收站：为 wallpapers 表扩展 deleted_at（软删除标记）
//!
//! ## 背景
//!
//! 删除壁纸由「物理删除」改为「移入回收站」两段式：先打软删除标记，
//! 过保留期或用户手动清空时才真正删除记录与磁盘文件。
//!
//! - `deleted_at`：NULL = 正常可见；非 NULL = 在回收站，值为移入时刻。
//!   沿用 `created_at` / `updated_at` 的字符串时间风格（`%Y-%m-%d %H:%M:%S`），
//!   不引入 DateTime 类型，避免污染既有 DTO 序列化格式。
//!
//! ## 文件保留策略
//!
//! 软删除阶段不移动 `wallpapers/` 与 `thumbnails/` 中的物理文件，仅打 DB 标记：
//! - 恢复退化为纯 DB 操作，无文件回搬的失败中间态；
//! - `file_path` 无需重写，不与 backup_service 的整目录打包冲突；
//! - 缩略图与壁纸文件同 stem 的约定（save_video_thumbnail 依赖）不被破坏。
//!
//! ## 索引
//!
//! 软删除后**每一处**壁纸查询都会带 `deleted_at IS NULL` 条件（主网格、
//! 智能收藏夹求值、轮播游标等），故为该列建索引，避免全表扫描。
//!
//! ## 兼容性
//!
//! 老用户升级：新列可空、无需回填，既有行全部为 NULL，语义等价于「无删除」，
//! 行为与升级前完全一致。

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // deleted_at：可空字符串时间戳，仅回收站内的壁纸填充
        manager
            .alter_table(
                Table::alter()
                    .table(Wallpapers::Table)
                    .add_column(ColumnDef::new(Wallpapers::DeletedAt).string().null())
                    .to_owned(),
            )
            .await?;

        // 所有壁纸查询都会过滤该列，建索引
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_wallpapers_deleted_at")
                    .table(Wallpapers::Table)
                    .col(Wallpapers::DeletedAt)
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
pub enum Wallpapers {
    Table,
    DeletedAt,
}
