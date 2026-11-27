//! 场景快照的存储层：只负责 scene_snapshots 表的 CRUD，不解析业务状态。

use anyhow::{bail, Context, Result};
use sea_orm::*;

use crate::entities::scene_snapshot;

/// 列出全部场景（按创建顺序，id 升序即可保证稳定）。
pub async fn get_all(db: &DatabaseConnection) -> Result<Vec<scene_snapshot::Model>> {
    let rows = scene_snapshot::Entity::find()
        .order_by_asc(scene_snapshot::Column::Id)
        .all(db)
        .await
        .context("Failed to list scene snapshots")?;
    Ok(rows)
}

pub async fn find_by_id(db: &DatabaseConnection, id: i32) -> Result<Option<scene_snapshot::Model>> {
    scene_snapshot::Entity::find_by_id(id)
        .one(db)
        .await
        .context("Failed to find scene snapshot")
}

pub async fn find_by_name(
    db: &DatabaseConnection,
    name: &str,
) -> Result<Option<scene_snapshot::Model>> {
    scene_snapshot::Entity::find()
        .filter(scene_snapshot::Column::Name.eq(name))
        .one(db)
        .await
        .context("Failed to find scene snapshot by name")
}

/// 保存当前状态；同名视为覆盖，保留原 created_at。
pub async fn save(
    db: &DatabaseConnection,
    name: &str,
    payload_json: &str,
) -> Result<scene_snapshot::Model> {
    if let Some(existing) = find_by_name(db, name).await? {
        let mut active: scene_snapshot::ActiveModel = existing.into();
        active.payload_json = Set(payload_json.to_string());
        return active
            .update(db)
            .await
            .context("Failed to overwrite scene snapshot");
    }

    let active = scene_snapshot::ActiveModel {
        name: Set(name.to_string()),
        payload_json: Set(payload_json.to_string()),
        ..Default::default()
    };
    active
        .insert(db)
        .await
        .context("Failed to insert scene snapshot")
}

pub async fn rename(db: &DatabaseConnection, id: i32, name: &str) -> Result<scene_snapshot::Model> {
    let existing = find_by_id(db, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("场景不存在"))?;
    // name 有唯一约束，重名直接拒绝而不是悄悄覆盖另一个场景。
    if let Some(other) = find_by_name(db, name).await? {
        if other.id != id {
            bail!("已存在同名场景");
        }
    }
    let mut active: scene_snapshot::ActiveModel = existing.into();
    active.name = Set(name.to_string());
    active
        .update(db)
        .await
        .context("Failed to rename scene snapshot")
}

pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<()> {
    scene_snapshot::Entity::delete_by_id(id)
        .exec(db)
        .await
        .context("Failed to delete scene snapshot")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use sea_orm::{Database, DatabaseConnection};
    use sea_orm_migration::MigratorTrait;

    use super::*;
    use crate::migration::Migrator;

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "mini-wallpaper-scene-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    async fn test_db(root: &TempDir) -> DatabaseConnection {
        let path = root.0.join("app.db");
        let db = Database::connect(format!("sqlite:{}?mode=rwc", path.display()))
            .await
            .unwrap();
        Migrator::up(&db, None).await.unwrap();
        db
    }

    #[tokio::test]
    async fn save_overwrites_same_name_and_keeps_created_at() {
        let root = TempDir::new();
        let db = test_db(&root).await;

        let first = save(&db, "工作", r#"{"displayMode":"independent"}"#)
            .await
            .unwrap();
        let created_at = first.created_at.clone();
        let second = save(&db, "工作", r#"{"displayMode":"mirror"}"#)
            .await
            .unwrap();

        assert_eq!(first.id, second.id);
        assert_eq!(second.created_at, created_at);
        assert!(second.payload_json.contains("mirror"));
        assert_eq!(get_all(&db).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn rename_rejects_duplicate_name() {
        let root = TempDir::new();
        let db = test_db(&root).await;
        let first = save(&db, "工作", "{}").await.unwrap();
        save(&db, "放松", "{}").await.unwrap();

        assert!(rename(&db, first.id, "放松").await.is_err());
        assert_eq!(
            find_by_id(&db, first.id).await.unwrap().unwrap().name,
            "工作"
        );
    }

    #[tokio::test]
    async fn delete_removes_only_the_target() {
        let root = TempDir::new();
        let db = test_db(&root).await;
        let first = save(&db, "工作", "{}").await.unwrap();
        let second = save(&db, "放松", "{}").await.unwrap();

        delete(&db, first.id).await.unwrap();
        let remaining = get_all(&db).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, second.id);
    }
}
