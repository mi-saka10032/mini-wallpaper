use sea_orm::entity::prelude::*;
use sea_orm::Set;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "scene_snapshots")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// 场景名，唯一；同名保存视为覆盖。
    pub name: String,
    /// `dto::scene_dto::SceneState` 的序列化结果，只含引用 id 与配置字段。
    pub payload_json: String,
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
