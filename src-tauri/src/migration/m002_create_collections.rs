use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Collections::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Collections::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Collections::Name).string().not_null())
                    .col(
                        ColumnDef::new(Collections::SortOrder)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Collections::CreatedAt)
                            .string()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Collections::UpdatedAt)
                            .string()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // 生产环境不支持回滚，如需变更请新建 migration
        Ok(())
    }
}

#[derive(DeriveIden)]
pub enum Collections {
    Table,
    Id,
    Name,
    SortOrder,
    CreatedAt,
    UpdatedAt,
}