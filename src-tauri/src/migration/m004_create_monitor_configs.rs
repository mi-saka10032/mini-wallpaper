use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(MonitorConfigs::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(MonitorConfigs::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(MonitorConfigs::MonitorId)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(MonitorConfigs::DisplayMode)
                            .string()
                            .not_null()
                            .default("independent"),
                    )
                    .col(ColumnDef::new(MonitorConfigs::WallpaperId).integer())
                    .col(ColumnDef::new(MonitorConfigs::CollectionId).integer())
                    .col(
                        ColumnDef::new(MonitorConfigs::FitMode)
                            .string()
                            .not_null()
                            .default("cover"),
                    )
                    .col(
                        ColumnDef::new(MonitorConfigs::PlayMode)
                            .string()
                            .not_null()
                            .default("sequential"),
                    )
                    .col(
                        ColumnDef::new(MonitorConfigs::PlayInterval)
                            .integer()
                            .not_null()
                            .default(300),
                    )
                    .col(
                        ColumnDef::new(MonitorConfigs::IsEnabled)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(MonitorConfigs::Active)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(MonitorConfigs::UpdatedAt)
                            .string()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 为关联字段创建索引，提升 JOIN/WHERE 查询性能
        manager
            .create_index(
                sea_orm_migration::prelude::Index::create()
                    .name("idx-monitor_configs-wallpaper_id")
                    .table(MonitorConfigs::Table)
                    .col(MonitorConfigs::WallpaperId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                sea_orm_migration::prelude::Index::create()
                    .name("idx-monitor_configs-collection_id")
                    .table(MonitorConfigs::Table)
                    .col(MonitorConfigs::CollectionId)
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
pub enum MonitorConfigs {
    Table,
    Id,
    MonitorId,
    DisplayMode,
    WallpaperId,
    CollectionId,
    FitMode,
    PlayMode,
    PlayInterval,
    IsEnabled,
    Active,
    UpdatedAt,
}