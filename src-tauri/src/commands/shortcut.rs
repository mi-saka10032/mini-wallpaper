use std::sync::Arc;

use log::warn;
use tauri::{Emitter, State};
use tokio::sync::Mutex;

use crate::ctx::AppContext;
use crate::runtime::carousel::{carousel_key, CarouselTask, ThumbnailChangedPayload};
use crate::runtime::Scheduler;
use crate::dto::shortcut_dto::{Direction, SwitchWallpaperRequest};
use crate::dto::Validated;
use crate::services::{collection_service, monitor_config_service};

/// 切换所有活跃显示器的壁纸（上一张/下一张）
#[tauri::command]
pub async fn switch_wallpaper(
    ctx: State<'_, AppContext>,
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
    req: Validated<SwitchWallpaperRequest>,
) -> Result<(), String> {
    let req = req.into_inner();

    // 获取所有 active 的 monitor_config
    let configs = monitor_config_service::get_all(&ctx.db)
        .await
        .map_err(|e| e.to_string())?;

    for config in configs {
        if !config.active {
            continue;
        }

        // 需要有 collection_id 才能切换
        let collection_id = match config.collection_id {
            Some(cid) => cid,
            None => continue,
        };

        let new_wid = match req.direction {
            Direction::Next => {
                collection_service::next_wallpaper_id(
                    &ctx.db,
                    collection_id,
                    config.wallpaper_id,
                    &config.play_mode,
                )
                .await
            }
            Direction::Prev => {
                collection_service::prev_wallpaper_id(
                    &ctx.db,
                    collection_id,
                    config.wallpaper_id,
                    &config.play_mode,
                )
                .await
            }
        }
        .map_err(|e| e.to_string())?;

        if let Some(wid) = new_wid {
            monitor_config_service::update_wallpaper_id(&ctx.db, &config.monitor_id, wid)
                .await
                .map_err(|e| e.to_string())?;

            // 1. 通知指定壁纸窗口更新壁纸
            let wm_guard = ctx.window_manager.lock().await;
            if let Err(e) = wm_guard.update_window(&config.monitor_id, wid) {
                warn!("[switch_wallpaper] 壁纸窗口更新失败: {}", e);
            }
            drop(wm_guard);

            // 2. 通知主窗口更新缩略图
            let _ = ctx.app_handle.emit(
                "thumbnail-changed",
                &ThumbnailChangedPayload {
                    monitor_id: config.monitor_id.clone(),
                    wallpaper_id: wid,
                },
            );

            // 3. 如果有运行中的定时器，重置计时
            let key = carousel_key(&config.monitor_id);
            let mut sched = scheduler.lock().await;
            if sched.is_running(&key) {
                sched.restart(
                    key,
                    CarouselTask {
                        app: ctx.app_handle.clone(),
                        monitor_id: config.monitor_id.clone(),
                    },
                );
            }
        }
    }

    Ok(())
}