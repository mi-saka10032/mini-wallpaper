// =============================================================================
// DEPRECATED: playlist 表已废弃
// =============================================================================
// 原设计中 playlist 用于管理播放列表，后改为 monitor_config 直接关联 collection。
// 表保留（不删 migration），但实体不再使用。
// 后续如需清理，可在新 migration 中 DROP TABLE playlists。
// =============================================================================

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "playlists")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub play_order: String,
    pub interval_sec: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
