use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Wallpapers::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Wallpapers::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Wallpapers::Name).string().not_null())
                    .col(ColumnDef::new(Wallpapers::Type).string().not_null())
                    .col(ColumnDef::new(Wallpapers::FilePath).string().not_null())
                    .col(ColumnDef::new(Wallpapers::ThumbPath).string())
                    .col(ColumnDef::new(Wallpapers::Width).integer())
                    .col(ColumnDef::new(Wallpapers::Height).integer())
                    .col(ColumnDef::new(Wallpapers::Duration).double())
                    .col(ColumnDef::new(Wallpapers::FileSize).big_integer())
                    .col(ColumnDef::new(Wallpapers::Tags).string())
                    .col(
                        ColumnDef::new(Wallpapers::IsFavorite)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Wallpapers::PlayCount)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Wallpapers::CreatedAt)
                            .string()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Wallpapers::UpdatedAt)
                            .string()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Wallpapers::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum Wallpapers {
    Table,
    Id,
    Name,
    Type,
    FilePath,
    ThumbPath,
    Width,
    Height,
    Duration,
    FileSize,
    Tags,
    IsFavorite,
    PlayCount,
    CreatedAt,
    UpdatedAt,
}
