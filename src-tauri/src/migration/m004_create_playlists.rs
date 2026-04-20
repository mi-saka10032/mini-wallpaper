use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Playlists::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Playlists::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Playlists::Name).string().not_null())
                    .col(
                        ColumnDef::new(Playlists::PlayOrder)
                            .string()
                            .not_null()
                            .default("sequential"),
                    )
                    .col(
                        ColumnDef::new(Playlists::IntervalSec)
                            .integer()
                            .not_null()
                            .default(1800),
                    )
                    .col(
                        ColumnDef::new(Playlists::CreatedAt)
                            .string()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Playlists::UpdatedAt)
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
            .drop_table(Table::drop().table(Playlists::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum Playlists {
    Table,
    Id,
    Name,
    PlayOrder,
    IntervalSec,
    CreatedAt,
    UpdatedAt,
}
