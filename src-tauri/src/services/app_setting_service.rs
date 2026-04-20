use anyhow::{Context, Result};
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};

use crate::entities::app_setting;

/// 获取所有设置
pub async fn get_all(db: &DatabaseConnection) -> Result<Vec<app_setting::Model>> {
    app_setting::Entity::find()
        .all(db)
        .await
        .context("Failed to fetch app settings")
}

/// 根据 key 获取单个设置值
pub async fn get(db: &DatabaseConnection, key: &str) -> Result<Option<String>> {
    let model = app_setting::Entity::find_by_id(key.to_string())
        .one(db)
        .await
        .context("Failed to fetch app setting")?;
    Ok(model.map(|m| m.value))
}

/// 设置键值对（存在则更新，不存在则插入）
pub async fn set(db: &DatabaseConnection, key: &str, value: &str) -> Result<app_setting::Model> {
    let existing = app_setting::Entity::find_by_id(key.to_string())
        .one(db)
        .await
        .context("Failed to fetch app setting")?;

    if let Some(existing) = existing {
        let mut active: app_setting::ActiveModel = existing.into();
        active.value = Set(value.to_string());
        let model = active
            .update(db)
            .await
            .context("Failed to update app setting")?;
        Ok(model)
    } else {
        let active = app_setting::ActiveModel {
            key: Set(key.to_string()),
            value: Set(value.to_string()),
        };
        let model = active
            .insert(db)
            .await
            .context("Failed to insert app setting")?;
        Ok(model)
    }
}
