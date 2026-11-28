//! 目录监听实体。
//!
//! P1 只落 schema 与实体定义，读写由 P2（同步服务）/ P4（生命周期）接入，
//! 在此之前尚未被业务引用，故显式允许死代码告警；接入后应移除本属性。
#![allow(dead_code)]

use sea_orm::entity::prelude::*;
use sea_orm::Set;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "watched_folders")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// canonicalize 后的绝对路径，全表唯一；失效后记录仍保留以便重选激活
    pub path: String,
    /// 目录名，用作自动创建收藏夹时的默认名称
    pub name: String,
    /// 关联的监听收藏夹；目录失效被清空后置 NULL
    pub collection_id: Option<i32>,
    /// 1 = 有效并参与扫描；0 = 路径失效、不扫描、弹窗提示
    pub valid: i32,
    pub last_scan_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    async fn before_save<C>(mut self, _db: &C, insert: bool) -> Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        let now = crate::utils::time::now_str();
        if insert {
            if self.created_at.is_not_set() {
                self.created_at = Set(now.clone());
            }
            if self.updated_at.is_not_set() {
                self.updated_at = Set(now);
            }
        } else if !self.updated_at.is_set() {
            self.updated_at = Set(now);
        }
        Ok(self)
    }
}
