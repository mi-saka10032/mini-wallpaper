//! 删除联动
//!
//! 壁纸删除、收藏夹删除、收藏夹移除壁纸等操作触发的联动逻辑，
//! 包括定时器管理、壁纸窗口通知、config 状态同步等。
//!
//! 所有方法通过 `impl Scheduler` 跨文件实现，
//! 天然拥有 `self.app`、`self.tasks` 等内部状态，零参数透传。
//!
//! ## 设计
//! - Command 层只需一行 `sched.on_xxx(...)` 调用，不再膨胀
//! - 内部自由调用 `self.stop()` / `self.restart()` 等生命周期方法
//! - 通过 `self.db()` / `self.window_manager()` 获取共享资源，无需外部传入

use std::collections::HashSet;

use log::{error, warn};

use super::Scheduler;
use crate::ctx::window_manager::WallpaperWindowManager;
use crate::dto::app_setting_dto::keys as setting_keys;
use crate::dto::shortcut_dto::Direction;
use crate::entities::monitor_config;
use crate::runtime::tasks::carousel::{carousel_key, CarouselTask};
use crate::services::{app_setting_service, collection_service, monitor_config_service};

impl Scheduler {
    // ==================== 公共入口方法（供 Command 层调用） ====================

    /// 从 DB 读取 display_mode 并判断是否为同步模式（mirror / extend）
    ///
    /// 同步模式下所有显示器共享同一壁纸，切换时需广播到所有壁纸窗口。
    pub async fn resolve_is_sync_mode(&self) -> bool {
        let db = self.db();
        let dm = app_setting_service::get(&db, setting_keys::DISPLAY_MODE)
            .await
            .unwrap_or(Some("independent".to_string()))
            .unwrap_or_else(|| "independent".to_string());
        dm == "mirror" || dm == "extend"
    }

    /// 壁纸批量删除后的联动处理
    ///
    /// 适用于 `delete_wallpapers` command，封装了：
    /// - 场景 1b：单张模式，壁纸被删 → 清空壁纸窗口
    /// - 场景 1c：收藏夹模式，当前壁纸被删 → 切换到下一张 + 重启定时器
    /// - 场景 1c 边界：收藏夹删后只剩 0 张 → 退化为清空
    /// - 场景 1d：收藏夹模式，删除的不是当前壁纸 → 检查剩余数量，≤1 则停止定时器
    /// - 场景 1e：display_mode 同步模式下广播到所有窗口
    ///
    /// `affected_configs`: 删除前预查的受影响 config 快照
    /// `deleted_ids`: 被删除的壁纸 ID 集合
    pub async fn on_wallpapers_deleted(
        &mut self,
        affected_configs: &[monitor_config::Model],
        deleted_ids: &HashSet<i32>,
    ) {
        if affected_configs.is_empty() {
            return;
        }

        let db = self.db();
        let is_sync = self.resolve_is_sync_mode().await;
        let wm = self.window_manager();
        let wm_guard = wm.lock().await;

        for config in affected_configs {
            let mid = &config.monitor_id;
            let key = carousel_key(mid);

            match config.collection_id {
                Some(cid) => {
                    // 收藏夹模式：当前壁纸被删，尝试切换到下一张或清空
                    Self::handle_current_wallpaper_removed_inner(
                        &db,
                        self,
                        &wm_guard,
                        config,
                        cid,
                        deleted_ids,
                        is_sync,
                    )
                    .await;
                }
                None => {
                    // 场景 1b：单张模式，壁纸被删 → 清空窗口 + 停止定时器
                    self.stop(&key);
                    wm_guard.notify_wallpaper_cleared(mid, is_sync);
                }
            }
        }

        // 场景 1d：检查未直接受影响的显示器的定时器状态
        let affected_mids: HashSet<String> = affected_configs
            .iter()
            .map(|c| c.monitor_id.clone())
            .collect();
        Self::check_orphaned_timers_inner(&db, self, &affected_mids).await;

        // 通知主窗口刷新 config 状态
        wm_guard.notify_config_refreshed();
    }

    /// 收藏夹删除后的联动处理
    ///
    /// 适用于 `delete_collection` command，封装了：
    /// - 场景 2a：显示器未引用该收藏夹 → 略过
    /// - 场景 2b：显示器引用该收藏夹且无 wallpaper_id → 清空壁纸窗口
    /// - 场景 2c：显示器引用该收藏夹但有 wallpaper_id → 回退单张模式，停止定时器
    /// - 场景 2d：display_mode 同步模式下广播到所有窗口
    ///
    /// `affected_configs`: 删除前预查的受影响 config 快照
    pub async fn on_collection_deleted(&mut self, affected_configs: &[monitor_config::Model]) {
        if affected_configs.is_empty() {
            return;
        }

        let is_sync = self.resolve_is_sync_mode().await;
        let wm = self.window_manager();
        let wm_guard = wm.lock().await;

        for config in affected_configs {
            let mid = &config.monitor_id;
            let key = carousel_key(mid);

            // 无论哪种子场景，定时器都需要停止（收藏夹已删除，轮播不再可能）
            self.stop(&key);

            match config.wallpaper_id {
                Some(_wid) => {
                    // 场景 2c：有 wallpaper_id → 回退单张模式，壁纸继续显示
                }
                None => {
                    // 场景 2b：无 wallpaper_id → 清空壁纸窗口
                    wm_guard.notify_wallpaper_cleared(mid, is_sync);
                }
            }
        }

        // 通知主窗口刷新 config 状态
        wm_guard.notify_config_refreshed();
    }

    /// 从收藏夹移除壁纸后的联动处理
    ///
    /// 适用于 `remove_wallpapers_from_collection` command，封装了：
    /// - 场景 3a：移除的壁纸不是当前播放的 → 检查剩余数量，≤1 则停止定时器
    /// - 场景 3b：移除的壁纸是当前播放的，且剩余 ≥1 张 → 切换到下一张 + 重启定时器
    /// - 场景 3c：移除的壁纸是当前播放的，且剩余 0 张 → 清空壁纸窗口
    /// - 场景 3d：display_mode 同步模式下广播到所有窗口
    ///
    /// `bound_configs`: 绑定该收藏夹的 config 列表
    /// `collection_id`: 收藏夹 ID
    /// `removing_ids`: 被移除的壁纸 ID 集合
    ///
    /// 返回 true 表示有 config 发生了变更（需要通知主窗口刷新）
    pub async fn on_wallpapers_removed_from_collection(
        &mut self,
        bound_configs: &[monitor_config::Model],
        collection_id: i32,
        removing_ids: &HashSet<i32>,
    ) -> bool {
        if bound_configs.is_empty() {
            return false;
        }

        let db = self.db();
        let is_sync = self.resolve_is_sync_mode().await;
        let wm = self.window_manager();
        let wm_guard = wm.lock().await;
        let mut config_changed = false;

        for config in bound_configs {
            let mid = &config.monitor_id;
            let key = carousel_key(mid);

            let current_wid_removed = config
                .wallpaper_id
                .map(|wid| removing_ids.contains(&wid))
                .unwrap_or(false);

            if current_wid_removed {
                // 当前播放的壁纸被移除，处理切换逻辑
                let changed = Self::handle_current_wallpaper_removed_inner(
                    &db,
                    self,
                    &wm_guard,
                    config,
                    collection_id,
                    removing_ids,
                    is_sync,
                )
                .await;
                config_changed = config_changed || changed;
            } else {
                // 场景 3a：当前壁纸未被移除，但收藏夹内容减少了
                if self.is_running(&key) {
                    Self::manage_timer_by_collection_inner(&db, self, mid, collection_id, false)
                        .await;
                }
            }
        }

        // 如果有 config 变更，通知主窗口刷新状态
        if config_changed {
            wm_guard.notify_config_refreshed();
        }

        config_changed
    }

    /// 切换指定显示器到相邻壁纸（上一张/下一张）并管理定时器
    ///
    /// 适用于 `switch_wallpaper` command，封装了：
    /// 1. 根据方向获取下一张/上一张壁纸 ID
    /// 2. 更新 DB 中的 wallpaper_id
    /// 3. 通知壁纸窗口 + 主窗口缩略图
    /// 4. 如果有运行中的定时器，重置计时
    ///
    /// 返回 true 表示成功切换了壁纸。
    pub async fn switch_to_adjacent_wallpaper(
        &mut self,
        config: &monitor_config::Model,
        direction: &Direction,
    ) -> bool {
        let db = self.db();
        let wm = self.window_manager();

        let collection_id = match config.collection_id {
            Some(cid) => cid,
            None => return false,
        };

        let new_wid = match direction {
            Direction::Next => {
                collection_service::next_wallpaper_id(
                    &db,
                    collection_id,
                    config.wallpaper_id,
                    &config.play_mode,
                )
                .await
            }
            Direction::Prev => {
                collection_service::prev_wallpaper_id(
                    &db,
                    collection_id,
                    config.wallpaper_id,
                    &config.play_mode,
                )
                .await
            }
        };

        let wid = match new_wid {
            Ok(Some(wid)) => wid,
            Ok(None) => return false,
            Err(e) => {
                warn!("[Scheduler] 获取相邻壁纸失败 {}: {}", config.monitor_id, e);
                return false;
            }
        };

        // 更新 DB
        if let Err(e) =
            monitor_config_service::update_wallpaper_id(&db, &config.monitor_id, wid).await
        {
            error!(
                "[Scheduler] 更新 wallpaper_id 失败 {}: {}",
                config.monitor_id, e
            );
            return false;
        }

        // 通知壁纸窗口 + 主窗口缩略图
        {
            let wm_guard = wm.lock().await;
            if let Err(e) = wm_guard.update_wallpaper(&config.monitor_id, wid) {
                warn!("[Scheduler] 壁纸更新通知失败 {}: {}", config.monitor_id, e);
            }
        }

        // 如果有运行中的定时器，重置计时
        let key = carousel_key(&config.monitor_id);
        if self.is_running(&key) {
            self.restart(
                key,
                CarouselTask {
                    monitor_id: config.monitor_id.clone(),
                },
            );
        }

        true
    }

    // ==================== 内部辅助方法 ====================

    /// 处理"当前播放的壁纸被移除"场景
    ///
    /// 逻辑：尝试从收藏夹获取下一张可用壁纸，
    /// - 有下一张 → 更新 DB + 通知窗口 + 管理定时器
    /// - 无下一张 → 先把 wallpaper_id 写空，成功后再清空窗口并停止定时器
    ///
    /// 返回 true 表示 config 发生了变更。写库失败时返回 false，窗口保持原状。
    ///
    /// 注意：使用 `&mut Scheduler` 而非 `&mut self` 以避免借用冲突
    async fn handle_current_wallpaper_removed_inner(
        db: &sea_orm::DatabaseConnection,
        sched: &mut Scheduler,
        wm: &WallpaperWindowManager,
        config: &monitor_config::Model,
        collection_id: i32,
        removed_ids: &HashSet<i32>,
        is_sync: bool,
    ) -> bool {
        let mid = &config.monitor_id;
        let key = carousel_key(mid);

        match persist_current_wallpaper_removal(db, config, collection_id, removed_ids).await {
            Some(CurrentWallpaperOutcome::Advanced(next_wid)) => {
                wm.notify_wallpaper_update(mid, next_wid, is_sync);
                Self::manage_timer_by_collection_inner(db, sched, mid, collection_id, true).await;
                true
            }
            Some(CurrentWallpaperOutcome::Cleared) => {
                sched.stop(&key);
                wm.notify_wallpaper_cleared(mid, is_sync);
                true
            }
            None => false,
        }
    }

    /// 根据收藏夹剩余壁纸数量管理定时器
    ///
    /// `force_restart`: true 表示需要重启（壁纸被强制切换），false 表示仅检查是否需要停止
    async fn manage_timer_by_collection_inner(
        db: &sea_orm::DatabaseConnection,
        sched: &mut Scheduler,
        monitor_id: &str,
        collection_id: i32,
        force_restart: bool,
    ) {
        let key = carousel_key(monitor_id);

        match collection_service::has_enough_wallpapers(db, collection_id).await {
            Ok(true) => {
                if force_restart {
                    sched.restart(
                        key,
                        CarouselTask {
                            monitor_id: monitor_id.to_string(),
                        },
                    );
                }
                // 非 force_restart 且有足够壁纸 → 定时器继续运行，不做操作
            }
            _ => {
                // 剩余 ≤1 张或查询失败，轮播无意义，停止定时器
                sched.stop(&key);
            }
        }
    }

    /// 检查未直接受影响的显示器的定时器状态
    ///
    /// 当前壁纸未被删除，但收藏夹内壁纸减少，可能导致定时器需要停止。
    async fn check_orphaned_timers_inner(
        db: &sea_orm::DatabaseConnection,
        sched: &mut Scheduler,
        affected_monitor_ids: &HashSet<String>,
    ) {
        let all_configs = monitor_config_service::get_all(db)
            .await
            .unwrap_or_default();

        for config in &all_configs {
            if affected_monitor_ids.contains(&config.monitor_id) {
                continue;
            }
            if let Some(cid) = config.collection_id {
                let key = carousel_key(&config.monitor_id);
                if sched.is_running(&key) {
                    Self::manage_timer_by_collection_inner(
                        db,
                        sched,
                        &config.monitor_id,
                        cid,
                        false,
                    )
                    .await;
                }
            }
        }
    }
}

enum CurrentWallpaperOutcome {
    Advanced(i32),
    Cleared,
}

/// 把「当前壁纸已不能继续播放」写进显示器配置。
///
/// 收藏夹里还有下一张时改写 `wallpaper_id`；没有下一张时把它置空。
/// 写库失败返回 `None`，调用方不得据此清空窗口。
async fn persist_current_wallpaper_removal(
    db: &sea_orm::DatabaseConnection,
    config: &monitor_config::Model,
    collection_id: i32,
    removed_ids: &HashSet<i32>,
) -> Option<CurrentWallpaperOutcome> {
    let mid = &config.monitor_id;
    match collection_service::next_wallpaper_id(
        db,
        collection_id,
        config.wallpaper_id,
        &config.play_mode,
    )
    .await
    {
        Ok(Some(next_wid)) if !removed_ids.contains(&next_wid) => {
            if let Err(e) = monitor_config_service::update_wallpaper_id(db, mid, next_wid).await {
                error!("[Scheduler] 更新 wallpaper_id 失败 {}: {}", mid, e);
                return None;
            }
            Some(CurrentWallpaperOutcome::Advanced(next_wid))
        }
        _ => {
            if let Err(e) = monitor_config_service::clear_wallpaper_id(db, mid).await {
                error!("[Scheduler] 清空 wallpaper_id 失败 {}: {}", mid, e);
                return None;
            }
            Some(CurrentWallpaperOutcome::Cleared)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicU64, Ordering};

    use sea_orm::{ActiveModelTrait, Database, DatabaseConnection, Set};
    use sea_orm_migration::MigratorTrait;

    use super::persist_current_wallpaper_removal;
    use crate::entities::{collection, monitor_config, wallpaper};
    use crate::migration::Migrator;
    use crate::services::{collection_service, monitor_config_service, wallpaper_service};

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "mini-wallpaper-removal-{}-{}",
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

    async fn insert_wallpaper(db: &DatabaseConnection, name: &str) -> wallpaper::Model {
        wallpaper::ActiveModel {
            name: Set(name.to_string()),
            r#type: Set("image".to_string()),
            file_path: Set(format!("wallpapers/{name}.png")),
            play_count: Set(0),
            created_at: Set("2026-01-01 00:00:00".to_string()),
            updated_at: Set("2026-01-01 00:00:00".to_string()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn insert_collection(db: &DatabaseConnection) -> collection::Model {
        collection::ActiveModel {
            name: Set("album".to_string()),
            sort_order: Set(0),
            created_at: Set("2026-01-01 00:00:00".to_string()),
            updated_at: Set("2026-01-01 00:00:00".to_string()),
            is_builtin: Set(0),
            kind: Set("manual".to_string()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn bind_monitor(
        db: &DatabaseConnection,
        collection_id: i32,
        wallpaper_id: i32,
    ) -> monitor_config::Model {
        monitor_config::ActiveModel {
            monitor_id: Set("display-1".to_string()),
            wallpaper_id: Set(Some(wallpaper_id)),
            collection_id: Set(Some(collection_id)),
            fit_mode: Set("cover".to_string()),
            play_mode: Set("sequential".to_string()),
            play_interval: Set(300),
            is_enabled: Set(true),
            active: Set(true),
            updated_at: Set("2026-01-01 00:00:00".to_string()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn removing_the_last_member_clears_wallpaper_id() {
        let root = TempDir::new();
        let db = test_db(&root).await;
        let album = insert_collection(&db).await;
        let image = insert_wallpaper(&db, "only").await;
        collection_service::add_wallpapers(&db, album.id, vec![image.id])
            .await
            .unwrap();
        let config = bind_monitor(&db, album.id, image.id).await;

        collection_service::remove_wallpapers(&db, album.id, vec![image.id])
            .await
            .unwrap();
        let removed = HashSet::from([image.id]);
        let outcome = persist_current_wallpaper_removal(&db, &config, album.id, &removed).await;

        assert!(matches!(
            outcome,
            Some(super::CurrentWallpaperOutcome::Cleared)
        ));
        let stored = monitor_config_service::get_by_monitor_id(&db, "display-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.wallpaper_id, None);
        assert_eq!(stored.collection_id, Some(album.id));
    }

    #[tokio::test]
    async fn playback_lookup_skips_trashed_wallpaper() {
        let root = TempDir::new();
        let db = test_db(&root).await;
        let image = insert_wallpaper(&db, "gone").await;

        wallpaper_service::trash_batch(&db, vec![image.id])
            .await
            .unwrap();

        assert!(wallpaper_service::get_active_by_id(&db, image.id)
            .await
            .unwrap()
            .is_none());
        assert!(wallpaper_service::get_by_id(&db, image.id)
            .await
            .unwrap()
            .is_some());
    }
}
