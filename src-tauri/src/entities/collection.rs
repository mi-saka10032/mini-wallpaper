use sea_orm::entity::prelude::*;
use sea_orm::Set;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collections")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
    /// 是否为系统内置收藏夹（1 = 内置，0 = 用户自建）
    ///
    /// 内置收藏夹由 m006 迁移种入，名为「我喜欢」，全表至多一条。
    /// 业务约束：不可重命名、不可删除（service 层守卫 + db unique partial index 兜底）。
    pub is_builtin: i32,
    /// 收藏夹类型：`manual`（手动）/ `smart`（智能收藏夹）。见 m009。
    ///
    /// 手动收藏夹成员走 `collection_wallpapers`；智能收藏夹成员由 `rule_json`
    /// 实时求值，不物化。默认 `manual`，内置「我喜欢」亦为手动。
    #[sea_orm(default_value = "manual")]
    pub kind: String,
    /// 智能收藏夹规则（结构化 JSON schema，防注入）。手动收藏夹为 NULL。
    pub rule_json: Option<String>,
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
        Ok(self)
    }
}
