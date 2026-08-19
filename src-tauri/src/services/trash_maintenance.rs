//! 回收站维护：过期项自动清理
//!
//! 回收站内的壁纸在超过保留期后被彻底删除（记录 + 关联 + 磁盘文件）。
//!
//! ## 触发时机
//!
//! 仅在**应用启动时做一次性扫描**，不引入常驻定时任务：
//! - 回收站清理对实时性无要求，延迟到下次启动完全可接受；
//! - 避免为一个低频维护动作占用一个长期运行的 timer。
//!
//! ## 配置
//!
//! - `trash_auto_purge`：是否启用自动清理，缺省视为启用（`true`）。
//!   用户显式设为 `false` 时回收站永久保留，仅能手动清空。
//! - `trash_retention_days`：保留天数，缺省 30 天。非法值回落到默认值，
//!   不因配置损坏而误删或崩溃。

use log::{info, warn};
use tauri::{AppHandle, Manager};

use crate::ctx::AppContext;
use crate::dto::app_setting_dto::{keys as setting_keys, DEFAULT_TRASH_RETENTION_DAYS};
use crate::services::{app_setting_service, wallpaper_service};

/// 启动时清理回收站中的过期壁纸
///
/// 全过程 best-effort：任何一步失败都只记录日志，绝不影响应用启动与可用性。
pub async fn purge_expired_on_startup(app: &AppHandle) {
    let Some(ctx) = app.try_state::<AppContext>() else {
        warn!("[Trash] AppContext 未就绪，跳过过期清理");
        return;
    };
    let db = ctx.db.clone();

    // 1. 是否启用自动清理（缺省启用）
    let auto_purge = match app_setting_service::get(&db, setting_keys::TRASH_AUTO_PURGE).await {
        Ok(Some(v)) => v != "false",
        Ok(None) => true,
        Err(e) => {
            warn!("[Trash] 读取 trash_auto_purge 失败，按启用处理: {}", e);
            true
        }
    };

    if !auto_purge {
        info!("[Trash] 自动清理已关闭，跳过过期扫描");
        return;
    }

    // 2. 保留天数（缺省 / 非法值回落默认）
    let retention_days = match app_setting_service::get(&db, setting_keys::TRASH_RETENTION_DAYS).await
    {
        Ok(Some(v)) => v.parse::<i64>().unwrap_or_else(|_| {
            warn!(
                "[Trash] trash_retention_days 值非法（{}），回落默认 {} 天",
                v, DEFAULT_TRASH_RETENTION_DAYS
            );
            DEFAULT_TRASH_RETENTION_DAYS
        }),
        Ok(None) => DEFAULT_TRASH_RETENTION_DAYS,
        Err(e) => {
            warn!(
                "[Trash] 读取 trash_retention_days 失败，回落默认 {} 天: {}",
                DEFAULT_TRASH_RETENTION_DAYS, e
            );
            DEFAULT_TRASH_RETENTION_DAYS
        }
    };

    // 3. 执行清理
    match wallpaper_service::purge_expired(&db, retention_days).await {
        Ok(0) => {
            info!("[Trash] 无过期壁纸需清理（保留 {} 天）", retention_days);
        }
        Ok(n) => {
            info!("[Trash] 已清理 {} 张过期壁纸（保留 {} 天）", n, retention_days);
        }
        Err(e) => {
            warn!("[Trash] 过期清理失败: {}", e);
        }
    }
}
