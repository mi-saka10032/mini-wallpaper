use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 1. 在 app_settings 中插入 display_mode 默认值
        let insert = Query::insert()
            .into_table(AppSettings::Table)
            .columns([AppSettings::Key, AppSettings::Value])
            .values_panic(["display_mode".into(), "independent".into()])
            .to_owned();

        manager.exec_stmt(insert).await?;

        // 2. SQLite 不支持 DROP COLUMN（3.35.0 之前），
        //    为兼容性考虑，保留 monitor_configs.display_mode 列但不再使用。
        //    新代码不再读写该列，旧数据自然废弃。

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[derive(DeriveIden)]
pub enum AppSettings {
    Table,
    Key,
    Value,
}
