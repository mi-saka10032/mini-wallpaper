//! 场景快照表：命名保存一整套桌面状态。
//!
//! ## 为什么单独建表
//!
//! 快照是「一组配置引用」而非设置项：把结构化 JSON 塞进 `app_settings` 的
//! 某个 KV 里，就没法单独命名 / 删除 / 排序，也无法被备份天然带走（KV 虽会
//! 带走，但语义和校验全混在一起）。单独建表后与既有 store / 命令体系一致，
//! 事务内恢复，`backup_service` 的整库快照自动包含它。
//!
//! payload_json 只存 monitor_configs 的业务字段与两个全局设置（display_mode
//! / global_volume），不存壁纸内容、不存主题外观、不存运行时牌堆。

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(SceneSnapshots::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SceneSnapshots::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(SceneSnapshots::Name)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(SceneSnapshots::PayloadJson)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SceneSnapshots::CreatedAt)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SceneSnapshots::UpdatedAt)
                            .string()
                            .not_null(),
                    )
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
enum SceneSnapshots {
    Table,
    Id,
    Name,
    PayloadJson,
    CreatedAt,
    UpdatedAt,
}
