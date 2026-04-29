//! 删除联动辅助模块（Command 层内部共享）
//!
//! 提取 delete_wallpapers / delete_collection / remove_wallpapers_from_collection
//! 三个 command 中的公共联动逻辑，消除重复代码。
//!
//! **设计原则**：
//! - 仅在 commands 模块内部使用（`pub(super)`），不对外暴露
//! - 接收已获取的 `&mut Scheduler` / `&WallpaperWindowManager` 等引用，不自行获取锁
//! - service 层保持纯函数 + 只传 db 的约束不变

use std::collections::HashSet;

use sea_orm::DatabaseConnection;

use crate::ctx::window_manager::WallpaperWindowManager;
use crate::dto::app_setting_dto::{self, keys as setting_keys};
use crate::runtime::carousel::{carousel_key, CarouselTask};
use crate::runtime::Scheduler;
use crate::services::{app_setting_service, collection_service, monitor_config_service};
use crate::entities::monitor_config;

/// 读取 display_mode 设置并判断是否为同步模式
///
/// 三个删除 command 中完全相同的逻辑，提取为公共函数。
pub(super) async fn is_sync_mode(db: &DatabaseConnection) -> bool {
    let dm = app_setting_service::get(db, setting_keys::DISPLAY_MODE)
        .await
        .unwrap_or(Some("independent".to_string()))
        .unwrap_or_else(|| "independent".to_string());
    app_setting_dto::is_sync_mode(&dm)
}

/// 处理"当前播放的壁纸被移除"场景
///
/// 适用于：
/// - delete_wallpapers 场景 1c / 1c 边界
/// - remove_wallpapers_from_collection 场景 3b / 3c
///
/// 逻辑：尝试从收藏夹获取下一张可用壁纸，
/// - 有下一张 → 更新 DB + 通知窗口 + 管理定时器
/// - 无下一张 → 清空窗口 + 停止定时器
///
/// 返回 true 表示 config 发生了变更（需要通知主窗口刷新）。
pub(super) async fn handle_current_wallpaper_removed(
    db: &DatabaseConnection,
    sched: &mut Scheduler,
    wm: &WallpaperWindowManager,
    config: &monitor_config::Model,
    collection_id: i32,
    removed_ids: &HashSet<i32>,
    is_sync: bool,
) -> bool {
    let mid = &config.monitor_id;
    let key = carousel_key(mid);

    match collection_service::next_wallpaper_id(
        db, collection_id, config.wallpaper_id, &config.play_mode,
    ).await {
        Ok(Some(next_wid)) if !removed_ids.contains(&next_wid) => {
            // 有下一张可用壁纸 → 更新 DB + 通知窗口
            if let Err(e) = monitor_config_service::update_wallpaper_id(db, mid, next_wid).await {
                log::error!("[PlaybackCascade] 更新 wallpaper_id 失败 {}: {}", mid, e);
                return false;
            }
            wm.notify_wallpaper_update(mid, next_wid, is_sync);

            // 检查收藏夹剩余数量，决定定时器策略
            manage_timer_by_collection(db, sched, mid, collection_id, true).await;
            true
        }
        _ => {
            // 收藏夹为空或下一张也在移除列表中 → 清空窗口
            sched.stop(&key);
            wm.notify_wallpaper_cleared(mid, is_sync);
            true
        }
    }
}

/// 根据收藏夹剩余壁纸数量管理定时器
///
/// 适用于：
/// - 壁纸切换后检查是否需要重启定时器
/// - 当前壁纸未被移除但收藏夹内容减少，检查定时器是否需要停止
///
/// `force_restart`: true 表示需要重启（壁纸被强制切换），false 表示仅检查是否需要停止
pub(super) async fn manage_timer_by_collection(
    db: &DatabaseConnection,
    sched: &mut Scheduler,
    monitor_id: &str,
    collection_id: i32,
    force_restart: bool,
) {
    let key = carousel_key(monitor_id);

    match collection_service::has_enough_wallpapers(db, collection_id).await {
        Ok(true) => {
            if force_restart {
                sched.restart(key, CarouselTask { monitor_id: monitor_id.to_string() });
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
/// 适用于 delete_wallpapers 场景 1d：
/// 当前壁纸未被删除，但收藏夹内壁纸减少，可能导致定时器需要停止。
///
/// `affected_monitor_ids`: 已处理过的显示器 ID 集合（跳过）
pub(super) async fn check_orphaned_timers(
    db: &DatabaseConnection,
    sched: &mut Scheduler,
    affected_monitor_ids: &HashSet<String>,
) {
    let all_configs = monitor_config_service::get_all(db).await.unwrap_or_default();

    for config in &all_configs {
        // 跳过已处理的
        if affected_monitor_ids.contains(&config.monitor_id) {
            continue;
        }
        if let Some(cid) = config.collection_id {
            let key = carousel_key(&config.monitor_id);
            if sched.is_running(&key) {
                manage_timer_by_collection(db, sched, &config.monitor_id, cid, false).await;
            }
        }
    }
}
