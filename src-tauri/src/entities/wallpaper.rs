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
    /// 来源标志：0 = 本地导入（AppData 内有文件副本）；1 = 外部目录监听（不复制源文件）
    ///
    /// 两类壁纸在业务上完全隔离，全部「本地壁纸库」查询走
    /// `wallpaper_service::local_only()` 统一过滤，禁止在各 service 手写条件。
    pub is_external: i32,
    /// 所属监听目录；本地导入壁纸恒为 NULL
    pub watched_folder_id: Option<i32>,
    /// 目录内相对文件名（lowercase），与 watched_folder_id 组成身份键
    pub source_key: Option<String>,
    /// 源文件字节数，用于判断内容是否变化
    pub source_size: Option<i64>,
    /// 源文件修改时间（unix 秒），与 source_size 一起判断内容是否变化
    pub source_mtime: Option<i64>,
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
