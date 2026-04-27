use sea_orm_migration::prelude::*;

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
                    .to_owned(),
            )
            .await?;

        // 为关联字段创建索引，提升 JOIN/WHERE 查询性能
        manager
            .create_index(
                sea_orm_migration::prelude::Index::create()
                    .name("idx-collection_wallpapers-collection_id")
                    .table(CollectionWallpapers::Table)
                    .col(CollectionWallpapers::CollectionId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                sea_orm_migration::prelude::Index::create()
                    .name("idx-collection_wallpapers-wallpaper_id")
                    .table(CollectionWallpapers::Table)
                    .col(CollectionWallpapers::WallpaperId)
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
pub enum CollectionWallpapers {
    Table,
    CollectionId,
    WallpaperId,
    SortOrder,
    AddedAt,
}