//! 删除联动
//!
//! 壁纸删除、收藏夹删除、收藏夹移除壁纸等操作触发的联动逻辑，
//! 包括定时器管理、壁纸窗口通知、config 状态同步等。
//!
//! 所有方法通过 `impl TaskRegistry` 跨文件实现，
//! 天然拥有 `self.app`、`self.tasks` 等内部状态，零参数透传。
//!
//! ## 设计
//! - Command 层只需一行 `sched.on_xxx(...)` 调用，不再膨胀
//! - 内部自由调用 `self.stop()` / `self.restart()` 等生命周期方法
//! - 通过 `self.db()` / `self.window_manager()` 获取共享资源，无需外部传入

use std::collections::HashSet;

use log::{error, warn};

use super::AppEffects;
use crate::dto::app_setting_dto::keys as setting_keys;
use crate::dto::shortcut_dto::Direction;
use crate::entities::monitor_config;
use crate::services::{app_setting_service, collection_service, monitor_config_service};
use crate::tasks::carousel::{carousel_key, CarouselTask};

impl AppEffects<'_> {
    // ==================== 公共入口方法（供 Command 层调用） ====================

    /// 一批删除联动写库完成后的统一收尾：一次 reconcile 覆盖全部受影响屏，
    /// 再通知主窗口刷新 config。scope 取 All，因为删除联动可能影响多屏。
    async fn finish_deletion_reconcile(&self) {
        let ctx = self.app_context();
        let mut guard = ctx.unified_manager.lock().await;
        if let Err(e) = guard
            .reconcile(crate::context::unified_manager::ReconcileScope::All)
            .await
        {
            warn!("[TaskRegistry] 删除联动 reconcile 失败: {}", e);
        }
        // 结构拉平后 notify_config_refreshed 已在同一个 manager 上，无需换锁。
        guard.notify_config_refreshed();
    }

    /// 从 DB 读取 display_mode 并判断是否为同步模式（mirror / extend）
    ///
    /// 同步模式下所有显示器共享同一壁纸，切换时需广播到所有壁纸窗口。
    pub async fn resolve_is_sync_mode(&self) -> bool {
        let db = self.db();
        let dm = app_setting_service::get(&db, setting_keys::DISPLAY_MODE)
            .await
            .unwrap_or(Some("independent".to_string()))
            .unwrap_or_else(|| "independent".to_string());
        monitor_config_service::is_sync_mode(&dm)
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

        // —— 阶段 1：DB 写入 + 定时器管理（无渲染锁） ——
        let db = self.db();
        // 同步组只决策一次：组内所有屏显示同一张壁纸，若逐条决策会让各组算出不同的
        // 「下一张」（play_mode=random）或只清一部分屏。以第一个受影响 config 为基准。
        let (targets, is_sync) = match monitor_config_service::active_targets(
            &db,
            &affected_configs[0].monitor_id,
        )
        .await
        {
            Ok(v) => v,
            Err(e) => {
                warn!("[TaskRegistry] 裁决同步组失败: {}", e);
                (Vec::new(), false)
            }
        };

        if is_sync {
            let base = &affected_configs[0];
            // 有收藏夹就尝试找下一张（只算一次），没有则整组清空显示。
            let next_wid = match base.collection_id {
                Some(cid) => {
                    match collection_service::next_wallpaper_id(
                        &db,
                        cid,
                        base.wallpaper_id,
                        &base.play_mode,
                    )
                    .await
                    {
                        Ok(Some(wid)) if !deleted_ids.contains(&wid) => Some(wid),
                        _ => None,
                    }
                }
                None => None,
            };
            match monitor_config_service::apply_group_wallpaper(&db, &base.monitor_id, next_wid)
                .await
            {
                Ok(_) => {
                    // 定时器按整组裁决：有下一张则重启组基准定时器，否则全组停止。
                    match (&base.collection_id, next_wid) {
                        (Some(cid), Some(_)) => {
                            Self::manage_timer_by_collection_inner(
                                &db,
                                self,
                                &base.monitor_id,
                                *cid,
                                true,
                            )
                            .await;
                        }
                        _ => {
                            for mid in &targets {
                                self.stop(&carousel_key(mid));
                            }
                        }
                    }
                }
                Err(e) => warn!("[TaskRegistry] 同步组删除联动写库失败: {}", e),
            }
        } else {
            for config in affected_configs {
                let mid = &config.monitor_id;
                let key = carousel_key(mid);

                match config.collection_id {
                    Some(cid) => {
                        Self::handle_current_wallpaper_removed_inner(
                            &db,
                            self,
                            config,
                            cid,
                            deleted_ids,
                        )
                        .await;
                    }
                    None => {
                        self.stop(&key);
                        // 无收藏夹时把 wallpaper_id 写空，交给 reconcile 派发清屏。
                        if let Err(e) = monitor_config_service::clear_wallpaper_id(&db, mid).await {
                            warn!("[TaskRegistry] 清空 wallpaper_id 失败 {}: {}", mid, e);
                        }
                    }
                }
            }
        }

        let affected_mids: HashSet<String> = if is_sync {
            targets.iter().cloned().collect()
        } else {
            affected_configs
                .iter()
                .map(|c| c.monitor_id.clone())
                .collect()
        };
        Self::check_orphaned_timers_inner(&db, self, &affected_mids).await;

        // —— 阶段 2：一次 reconcile 统一派发全部窗口副作用 ——
        self.finish_deletion_reconcile().await;
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

        // 定时器管理：collection_service::delete 已置空 monitor_config.collection_id，
        // 这里只停定时器；壁纸窗口的清屏/切换由 reconcile 从 DB diff 出来统一派发。
        for config in affected_configs {
            self.stop(&carousel_key(&config.monitor_id));
        }

        // —— 一次 reconcile 统一派发全部窗口副作用 ——
        self.finish_deletion_reconcile().await;
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

        // —— 阶段 1：DB 写入 + 定时器管理（无渲染锁） ——
        let db = self.db();
        // 仅表示 DB 的 wallpaper_id 是否被改写，决定是否让主窗口刷新缩略图。
        let mut config_changed = false;

        // 同步组只决策一次：逐条处理会让组内各屏算出不同「下一张」或只清一部分屏。
        let (targets, is_sync) =
            match monitor_config_service::active_targets(&db, &bound_configs[0].monitor_id).await {
                Ok(v) => v,
                Err(e) => {
                    warn!("[TaskRegistry] 裁决同步组失败: {}", e);
                    (Vec::new(), false)
                }
            };

        if is_sync {
            let base = &bound_configs[0];
            let current_wid_removed = base
                .wallpaper_id
                .map(|wid| removing_ids.contains(&wid))
                .unwrap_or(false);
            if current_wid_removed {
                // 收藏夹里还有可用下一张就整组切过去；没有则整组清空显示。
                let next_wid = match collection_service::next_wallpaper_id(
                    &db,
                    collection_id,
                    base.wallpaper_id,
                    &base.play_mode,
                )
                .await
                {
                    Ok(Some(wid)) if !removing_ids.contains(&wid) => Some(wid),
                    _ => None,
                };
                match monitor_config_service::apply_group_wallpaper(&db, &base.monitor_id, next_wid)
                    .await
                {
                    Ok(_) => {
                        config_changed = true;
                        if next_wid.is_some() {
                            Self::manage_timer_by_collection_inner(
                                &db,
                                self,
                                &base.monitor_id,
                                collection_id,
                                true,
                            )
                            .await;
                        } else {
                            for mid in &targets {
                                self.stop(&carousel_key(mid));
                            }
                        }
                    }
                    Err(e) => warn!("[TaskRegistry] 同步组移除联动写库失败: {}", e),
                }
            } else if self.is_running(&carousel_key(&base.monitor_id)) {
                Self::manage_timer_by_collection_inner(
                    &db,
                    self,
                    &base.monitor_id,
                    collection_id,
                    false,
                )
                .await;
            }
        } else {
            for config in bound_configs {
                let mid = &config.monitor_id;
                let key = carousel_key(mid);

                let current_wid_removed = config
                    .wallpaper_id
                    .map(|wid| removing_ids.contains(&wid))
                    .unwrap_or(false);

                if current_wid_removed {
                    let changed = Self::handle_current_wallpaper_removed_inner(
                        &db,
                        self,
                        config,
                        collection_id,
                        removing_ids,
                    )
                    .await;
                    config_changed = config_changed || changed;
                } else if self.is_running(&key) {
                    Self::manage_timer_by_collection_inner(&db, self, mid, collection_id, false)
                        .await;
                }
            }
        }

        // —— 阶段 2：一次 reconcile 统一派发全部窗口副作用 ——
        if config_changed {
            self.finish_deletion_reconcile().await;
        }

        config_changed
    }

    /// 切换指定显示器到相邻壁纸（上一张/下一张）并管理定时器
    ///
    /// 适用于托盘菜单 / 全局快捷键，封装了：
    /// 1. 根据方向获取下一张/上一张壁纸 ID（同步组只算一次）
    /// 2. 单事务写全组 wallpaper_id 并全量对齐组内其它配置
    /// 3. 一次 reconcile 统一通知壁纸窗口 + 主窗口缩略图
    /// 4. 如果有运行中的定时器，重置计时
    ///
    /// 返回 true 表示成功切换了壁纸。
    pub async fn switch_to_adjacent_wallpaper(
        &mut self,
        config: &monitor_config::Model,
        direction: &Direction,
    ) -> bool {
        let db = self.db();

        if config.collection_id.is_none() {
            return false;
        }

        // 统一用例按 play_mode 分流：random 消费环形牌堆、sequential 走原游标。
        // 这里只消费不重洗——写库成功后 DB 的 id 正好是 cursor 那张，
        // 下次切换仍在同一条序列上，Prev 才能回退到刚才那张。
        let new_wid =
            crate::application::playlist::pick_adjacent(self.ctx, &db, config, direction).await;

        let wid = match new_wid {
            Ok(Some(wid)) => wid,
            Ok(None) => return false,
            Err(e) => {
                warn!(
                    "[TaskRegistry] 获取相邻壁纸失败 {}: {}",
                    config.monitor_id, e
                );
                return false;
            }
        };

        // 组写入统一收在 service：只算一次 wid（play_mode=random 时整组必须拿到同一张），
        // 由 apply_group_wallpaper 单事务写全组 + 全量对齐，并回传 is_sync 决定 reconcile 范围。
        let (_, is_sync) =
            match monitor_config_service::apply_group_wallpaper(&db, &config.monitor_id, Some(wid))
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    error!("[TaskRegistry] 组写入失败 {}: {}", config.monitor_id, e);
                    return false;
                }
            };

        // 单次 reconcile：同步组用 All，一次看到全量新状态，避免逐屏 attach 错开帧时钟。
        // 收窄作用域：reconcile 结束后立即释放对 self 的借用，后面 restart 需要 &mut self。
        {
            let scope = if is_sync {
                crate::context::unified_manager::ReconcileScope::All
            } else {
                crate::context::unified_manager::ReconcileScope::One(config.monitor_id.clone())
            };
            let ctx = self.app_context();
            let mut guard = ctx.unified_manager.lock().await;
            if let Err(e) = guard.reconcile(scope).await {
                warn!("[TaskRegistry] 删除联动 reconcile 失败: {}", e);
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
    /// 架构重整 Phase 5：本 helper 只负责写 DB + 管定时器，不再收集渲染通知；
    /// 渲染副作用由外层公共入口在数据落库后调一次 reconcile 统一派发。
    /// 返回 true 表示 config 发生了变更。写库失败时返回 false。
    ///
    /// 注意：使用 `&mut TaskRegistry` 而非 `&mut self` 以避免借用冲突
    async fn handle_current_wallpaper_removed_inner(
        db: &sea_orm::DatabaseConnection,
        sched: &mut AppEffects<'_>,
        config: &monitor_config::Model,
        collection_id: i32,
        removed_ids: &HashSet<i32>,
    ) -> bool {
        let mid = &config.monitor_id;
        let key = carousel_key(mid);

        match persist_current_wallpaper_removal(db, config, collection_id, removed_ids).await {
            Some(CurrentWallpaperOutcome::Advanced) => {
                Self::manage_timer_by_collection_inner(db, sched, mid, collection_id, true).await;
                true
            }
            Some(CurrentWallpaperOutcome::Cleared) => {
                sched.stop(&key);
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
        sched: &mut AppEffects<'_>,
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
        sched: &mut AppEffects<'_>,
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

/// 「当前壁纸已不能继续播放」的落库结果。
/// 渲染副作用不再由本模块决定：reconcile 会从 DB diff 出 wallpaper_id 的变更。
enum CurrentWallpaperOutcome {
    /// 收藏夹里还有下一张，DB 已写入新 wallpaper_id。
    Advanced,
    /// 无可用下一张，DB 的 wallpaper_id 已置空。
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
                error!("[TaskRegistry] 更新 wallpaper_id 失败 {}: {}", mid, e);
                return None;
            }
            Some(CurrentWallpaperOutcome::Advanced)
        }
        _ => {
            if let Err(e) = monitor_config_service::clear_wallpaper_id(db, mid).await {
                error!("[TaskRegistry] 清空 wallpaper_id 失败 {}: {}", mid, e);
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
        // get_by_id 已注释，不再断言回收站记录仍可被它读到。
        // assert!(wallpaper_service::get_by_id(&db, image.id)
        //     .await
        //     .unwrap()
        //     .is_some());
    }
}
