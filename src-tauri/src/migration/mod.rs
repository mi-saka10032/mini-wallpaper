pub mod m001_create_wallpapers;
pub mod m002_create_collections;
pub mod m003_create_collection_wallpapers;
pub mod m004_create_monitor_configs;
pub mod m005_create_app_settings;
pub mod m006_seed_builtin_collections;
pub mod m007_drop_is_favorite;
pub mod m008_create_tag_system;
pub mod m009_add_smart_collection;

use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m001_create_wallpapers::Migration),
            Box::new(m002_create_collections::Migration),
            Box::new(m003_create_collection_wallpapers::Migration),
            Box::new(m004_create_monitor_configs::Migration),
            Box::new(m005_create_app_settings::Migration),
            Box::new(m006_seed_builtin_collections::Migration),
            Box::new(m007_drop_is_favorite::Migration),
            Box::new(m008_create_tag_system::Migration),
            Box::new(m009_add_smart_collection::Migration),
        ]
    }
}