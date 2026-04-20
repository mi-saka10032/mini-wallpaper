use sea_orm_migration::prelude::*;

use super::m001_create_wallpapers::Wallpapers;
use super::m004_create_playlists::Playlists;

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
                    .col(ColumnDef::new(MonitorConfigs::PlaylistId).integer())
                    .col(
                        ColumnDef::new(MonitorConfigs::FitMode)
                            .string()
                            .not_null()
                            .default("cover"),
                    )
                    .col(
                        ColumnDef::new(MonitorConfigs::UpdatedAt)
                            .string()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(MonitorConfigs::Table, MonitorConfigs::WallpaperId)
                            .to(Wallpapers::Table, Wallpapers::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(MonitorConfigs::Table, MonitorConfigs::PlaylistId)
                            .to(Playlists::Table, Playlists::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(MonitorConfigs::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum MonitorConfigs {
    Table,
    Id,
    MonitorId,
    DisplayMode,
    WallpaperId,
    PlaylistId,
    FitMode,
    UpdatedAt,
}
