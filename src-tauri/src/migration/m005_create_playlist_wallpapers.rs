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
                    .table(PlaylistWallpapers::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PlaylistWallpapers::PlaylistId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(PlaylistWallpapers::WallpaperId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(PlaylistWallpapers::SortOrder)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .primary_key(
                        Index::create()
                            .col(PlaylistWallpapers::PlaylistId)
                            .col(PlaylistWallpapers::WallpaperId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(
                                PlaylistWallpapers::Table,
                                PlaylistWallpapers::PlaylistId,
                            )
                            .to(Playlists::Table, Playlists::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(
                                PlaylistWallpapers::Table,
                                PlaylistWallpapers::WallpaperId,
                            )
                            .to(Wallpapers::Table, Wallpapers::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(PlaylistWallpapers::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum PlaylistWallpapers {
    Table,
    PlaylistId,
    WallpaperId,
    SortOrder,
}
