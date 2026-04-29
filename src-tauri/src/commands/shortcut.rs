use std::sync::Arc;

use tauri::State;
use tokio::sync::Mutex;

use crate::ctx::AppContext;
use crate::runtime::Scheduler;
use crate::dto::shortcut_dto::SwitchWallpaperRequest;
use crate::dto::Validated;
use crate::services::monitor_config_service;

use super::error::CommandResult;

/// 切换所有活跃显示器的壁纸（上一张/下一张）
///
/// 遍历所有 active 且绑定了收藏夹的显示器配置，
/// 委托 `Scheduler::switch_to_adjacent_wallpaper` 执行：
/// 获取相邻壁纸 → 更新 DB → 通知窗口 → 重置定时器。
#[tauri::command]
pub async fn switch_wallpaper(
    ctx: State<'_, AppContext>,
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
    req: Validated<SwitchWallpaperRequest>,
) -> CommandResult<()> {
    let req = req.into_inner();
    let configs = monitor_config_service::get_all(&ctx.db).await?;

    let mut sched = scheduler.lock().await;

    for config in &configs {
        if !config.active {
            continue;
        }
        sched.switch_to_adjacent_wallpaper(config, &req.direction).await;
    }

    Ok(())
}
