use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "monitor_configs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// 显示器标识（系统 monitor id）
    pub monitor_id: String,
    /// 固定壁纸 ID（与 collection_id 二选一）
    pub wallpaper_id: Option<i32>,
    /// 关联收藏夹 ID（轮播该收藏夹内壁纸，与 wallpaper_id 二选一）
    pub collection_id: Option<i32>,
    /// 壁纸适配模式：cover / contain / fill / stretch / center
    pub fit_mode: String,
    /// 播放模式：sequential / random
    pub play_mode: String,
    /// 轮播间隔（秒），默认 300
    pub play_interval: i32,
    /// 是否启用轮播
    pub is_enabled: bool,
    /// 显示器是否当前物理可用（前端检测后设置）
    pub active: bool,
    pub updated_at: String,
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
        belongs_to = "super::collection::Entity",
        from = "Column::CollectionId",
        to = "super::collection::Column::Id"
    )]
    Collection,
}

impl Related<super::wallpaper::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Wallpaper.def()
    }
}

impl Related<super::collection::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Collection.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}