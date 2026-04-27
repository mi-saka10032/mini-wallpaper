use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AppSettings::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AppSettings::Key)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(AppSettings::Value).string().not_null())
                    .to_owned(),
            )
            .await?;

        // 插入默认设置
        let insert = Query::insert()
            .into_table(AppSettings::Table)
            .columns([AppSettings::Key, AppSettings::Value])
            .values_panic(["theme".into(), "dark".into()])
            .values_panic(["language".into(), "zh-CN".into()])
            .values_panic(["pause_on_fullscreen".into(), "true".into()])
            .values_panic(["global_volume".into(), "0".into()])
            .to_owned();

        manager.exec_stmt(insert).await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // 生产环境不支持回滚，如需变更请新建 migration
        Ok(())
    }
}

#[derive(DeriveIden)]
pub enum AppSettings {
    Table,
    Key,
    Value,
}
