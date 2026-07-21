//! 收藏切换联动（FavoriteEffects）
//!
//! 把"收藏 / 取消收藏某张壁纸"抽象为对内置「我喜欢」收藏夹的
//! add / remove 原子操作，并统一处理三件事：
//!
//! 1. **归属切换**：动态定位内置收藏夹（禁止 hardcode id），
//!    已在夹内则移除，否则加入（幂等）。
//! 2. **移除联动**：取消收藏可能影响正在轮播「我喜欢」的显示器
//!    （当前壁纸被移出需切换 / 停表 / 清屏），复用既有的
//!    [`Scheduler::on_wallpapers_removed_from_collection`]，零逻辑重复。
//! 3. **事件广播**：发送 `favorites-changed`，前端据此刷新收藏
//!    id 集合，红心按钮亮灭与「我喜欢」列表即时同步。
//!
//! ## 三方调用方共享
//!
//! - 全局快捷键 / 托盘：作用于"当前显示壁纸"（`dispatch_toggle_favorite` 解析 id）
//! - 前端红心按钮 / 右键菜单：作用于"卡片壁纸"（`toggle_favorite` command 透传 id）
//!
//! 两条路径最终都汇聚到本文件的 [`Scheduler::toggle_favorite`] 单一入口。

use std::collections::HashSet;

use anyhow::{anyhow, Result};
use log::warn;

use super::Scheduler;
use crate::events::{FavoritesChangedPayload, TypedEmit};
use crate::services::{collection_service, monitor_config_service};

impl Scheduler {
    /// 切换某张壁纸在内置「我喜欢」收藏夹中的归属
    ///
    /// 返回 `Ok(true)` 表示切换后为"已收藏"，`Ok(false)` 表示"已取消收藏"。
    ///
    /// 取消收藏时，若该壁纸正被某显示器轮播显示，会复用删除联动逻辑
    /// 保证壁纸窗口 / 定时器状态一致。
    pub async fn toggle_favorite(&mut self, wallpaper_id: i32) -> Result<bool> {
        let db = self.db();

        // 1. 动态定位内置收藏夹（禁止 hardcode id）
        let builtin = collection_service::find_builtin(&db)
            .await?
            .ok_or_else(|| anyhow!("内置「我喜欢」收藏夹不存在（m006 迁移未生效？）"))?;
        let collection_id = builtin.id;

        // 2. 探测当前归属，决定 add / remove
        let already =
            collection_service::is_wallpaper_in_collection(&db, collection_id, wallpaper_id)
                .await?;

        let favorited = if already {
            // ---- 取消收藏 ----
            // 预查绑定该收藏夹的 config，用于移除后的定时器 / 窗口联动
            let bound_configs =
                monitor_config_service::get_configs_by_collection_id(&db, collection_id).await?;

            let removed = collection_service::remove_wallpapers(
                &db,
                collection_id,
                vec![wallpaper_id],
            )
            .await?;

            if removed > 0 {
                let removing_ids: HashSet<i32> = std::iter::once(wallpaper_id).collect();
                self.on_wallpapers_removed_from_collection(
                    &bound_configs,
                    collection_id,
                    &removing_ids,
                )
                .await;
            }
            false
        } else {
            // ---- 加入收藏 ----
            collection_service::add_wallpapers(&db, collection_id, vec![wallpaper_id]).await?;
            true
        };

        // 3. 广播事件，前端据此刷新收藏状态
        if let Err(e) = self.app.typed_emit(&FavoritesChangedPayload {
            wallpaper_id,
            favorited,
        }) {
            warn!("[FavoriteEffects] 发送 favorites-changed 事件失败: {}", e);
        }

        Ok(favorited)
    }
}
