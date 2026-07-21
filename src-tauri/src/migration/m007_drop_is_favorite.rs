//! 清理 `wallpapers.is_favorite` 僵尸字段
//!
//! ## 背景
//!
//! - 收藏语义早已由 `collections` + `collection_wallpapers` 多对多承载，
//!   m006 又落地了「我喜欢」内置收藏夹（`is_builtin = 1`）作为快捷收藏落点。
//! - `wallpapers.is_favorite` 自项目早期以来从未被任何业务代码读写，
//!   插入时永远写死 0，是彻底的"僵尸字段"。本迁移将其删除。
//!
//! ## 设计要点
//!
//! - SQLite 自 3.35.0（2021-03）起原生支持 `ALTER TABLE ... DROP COLUMN`，
//!   所有当前可用的 Tauri 平台均满足。
//! - `is_favorite` 无索引、无约束、无外键引用，可直接安全删除。
//! - 迁移由 `seaql_migrations` 表记录版本，每个迁移仅执行一次，
//!   无需额外的幂等兜底。
//!
//! ## 兼容性
//!
//! - 老用户升级：删除既有列，历史值全为 0，无信息丢失。
//! - 新装应用：m001 建表后本迁移紧接着删列（实际等价于该列从未存在）。

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SeaQuery 的 drop_column DSL 在部分版本对 SQLite 的支持不稳定，
        // 这里直接用原生 SQL 发起标准 `ALTER TABLE ... DROP COLUMN`
        // （SQLite 3.35.0+ 原生支持）。
        let backend = manager.get_database_backend();
        manager
            .get_connection()
            .execute(sea_orm::Statement::from_string(
                backend,
                "ALTER TABLE wallpapers DROP COLUMN is_favorite".to_string(),
            ))
            .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // 生产环境不支持回滚，如需变更请新建 migration
        Ok(())
    }
}