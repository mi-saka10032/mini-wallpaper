use sea_orm::entity::prelude::*;
use sea_orm::Set;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "wallpapers")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    #[sea_orm(column_name = "type")]
    pub r#type: String,
    pub file_path: String,
    pub thumb_path: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub duration: Option<f64>,
    pub file_size: Option<i64>,
    pub play_count: i32,
    pub created_at: String,
    pub updated_at: String,
    /// 回收站标记：None = 正常可见；Some(时刻) = 已移入回收站
    pub deleted_at: Option<String>,
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
            // 仅 NotSet 时填充，保留调用方显式 Set 的测试哨兵值
            if self.created_at.is_not_set() {
                self.created_at = Set(now.clone());
            }
            if self.updated_at.is_not_set() {
                self.updated_at = Set(now);
            }
        } else if !self.updated_at.is_set() {
            // update 时 ActiveModel 多来自 Model::into()，updated_at 为 Unchanged（非 NotSet）。
            // 除显式 Set（哨兵值）外一律刷新，否则 updated_at 永远不会更新。
            self.updated_at = Set(now);
        }
        // deleted_at 是软删除业务标记，由 service 显式管理，不在此自动填充
        Ok(self)
    }
}
