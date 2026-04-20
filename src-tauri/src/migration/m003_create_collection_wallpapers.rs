use sea_orm_migration::prelude::*;

use super::m001_create_wallpapers::Wallpapers;
use super::m002_create_collections::Collections;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CollectionWallpapers::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CollectionWallpapers::CollectionId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CollectionWallpapers::WallpaperId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CollectionWallpapers::SortOrder)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(CollectionWallpapers::AddedAt)
                            .string()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .primary_key(
                        Index::create()
                            .col(CollectionWallpapers::CollectionId)
                            .col(CollectionWallpapers::WallpaperId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(
                                CollectionWallpapers::Table,
                                CollectionWallpapers::CollectionId,
                            )
                            .to(Collections::Table, Collections::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(
                                CollectionWallpapers::Table,
                                CollectionWallpapers::WallpaperId,
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
            .drop_table(Table::drop().table(CollectionWallpapers::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum CollectionWallpapers {
    Table,
    CollectionId,
    WallpaperId,
    SortOrder,
    AddedAt,
}
