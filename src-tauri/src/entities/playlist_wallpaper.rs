// =============================================================================
// DEPRECATED: playlist_wallpapers 表已废弃
// =============================================================================
// 原设计中用于 playlist 与 wallpaper 的多对多关联。
// 后改为 monitor_config 直接关联 collection，此表不再使用。
// 表保留（不删 migration），但实体不再使用。
// =============================================================================

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "playlist_wallpapers")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub playlist_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub wallpaper_id: i32,
    pub sort_order: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::playlist::Entity",
        from = "Column::PlaylistId",
        to = "super::playlist::Column::Id"
    )]
    Playlist,
    #[sea_orm(
        belongs_to = "super::wallpaper::Entity",
        from = "Column::WallpaperId",
        to = "super::wallpaper::Column::Id"
    )]
    Wallpaper,
}

impl Related<super::playlist::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Playlist.def()
    }
}

impl Related<super::wallpaper::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Wallpaper.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
