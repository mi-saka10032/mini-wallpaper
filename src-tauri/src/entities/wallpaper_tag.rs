use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "wallpaper_tags")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub wallpaper_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub tag_id: i32,
    pub created_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::wallpaper::Entity",
        from = "Column::WallpaperId",
        to = "super::wallpaper::Column::Id"
    )]
    Wallpaper,
    #[sea_orm(
        belongs_to = "super::tag::Entity",
        from = "Column::TagId",
        to = "super::tag::Column::Id"
    )]
    Tag,
}

impl Related<super::wallpaper::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Wallpaper.def()
    }
}

impl Related<super::tag::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Tag.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
