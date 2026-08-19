//! 智能收藏夹：为 collections 表扩展 kind + rule_json
//!
//! ## 背景
//!
//! 智能收藏夹（方案 A1）本质是「基于复合规则对 wallpaper 表的复合查询」，
//! 不物化成员，命中集实时求值。为在既有 collections 表上区分两类收藏夹并
//! 承载规则，新增两列：
//!
//! - `kind`：`manual`（手动收藏夹，既有语义）/ `smart`（智能收藏夹）。
//!   默认 `manual`，老数据与内置「我喜欢」自动归为手动，行为不变。
//! - `rule_json`：智能收藏夹的规则（结构化 JSON，见规则引擎）。手动收藏夹为 NULL。
//!   出于安全（防注入）与心智负担，规则以 JSON schema 存储，绝不存裸类 SQL。
//!
//! ## 兼容性
//!
//! - 老用户升级：两列带默认值 / 可空，既有行无需回填，语义等价于全部为手动收藏夹。
//! - 手动收藏夹的成员关系仍走 `collection_wallpapers`，与本迁移互不干扰。

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // kind：NOT NULL DEFAULT 'manual'
        manager
            .alter_table(
                Table::alter()
                    .table(Collections::Table)
                    .add_column(
                        ColumnDef::new(Collections::Kind)
                            .string()
                            .not_null()
                            .default("manual"),
                    )
                    .to_owned(),
            )
            .await?;

        // rule_json：可空 TEXT（仅智能收藏夹填充）
        manager
            .alter_table(
                Table::alter()
                    .table(Collections::Table)
                    .add_column(ColumnDef::new(Collections::RuleJson).text().null())
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
pub enum Collections {
    Table,
    Kind,
    RuleJson,
}
