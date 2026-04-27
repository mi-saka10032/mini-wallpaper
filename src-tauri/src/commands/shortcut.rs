use log::warn;
use tauri::{Emitter, Manager, State};

use crate::services::{collection_service, monitor_config_service};
use crate::services::timer_manager::{TimerManagerState, ThumbnailChangedPayload};
use sea_orm::DatabaseConnection;

/// 切换壁纸方向
#[derive(serde::Deserialize)]
pub enum Direction {
    #[serde(rename = "next")]
    Next,
    #[serde(rename = "prev")]
    Prev,
}

/// 切换所有活跃显示器的壁纸（上一张/下一张）
#[tauri::command]
pub async fn switch_wallpaper(
    direction: Direction,
    db: State<'_, DatabaseConnection>,
    timer_state: State<'_, TimerManagerState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    // 获取所有 active 的 monitor_config
    let configs = monitor_config_service::get_all(&db)
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
        }
        .map_err(|e| e.to_string())?;

        if let Some(wid) = new_wid {
            monitor_config_service::update_wallpaper_id(&db, &config.monitor_id, wid)
                .await
                .map_err(|e| e.to_string())?;

            // 1. 通知指定壁纸窗口更新壁纸
            let wm_state = app_handle.state::<crate::services::wallpaper_window_service::WallpaperWindowManagerState>();
            let wm_guard = wm_state.lock().await;
            if let Err(e) = wm_guard.update_window(&app_handle, &config.monitor_id, wid) {
                warn!("[switch_wallpaper] 壁纸窗口更新失败: {}", e);
            }
            drop(wm_guard);

            // 2. 通知主窗口更新缩略图
            let _ = app_handle.emit(
                "thumbnail-changed",
                &ThumbnailChangedPayload {
                    monitor_id: config.monitor_id.clone(),
                    wallpaper_id: wid,
                },
            );

            // 3. 如果有运行中的定时器，重置计时
            let mut manager = timer_state.lock().await;
            if manager.is_running(&config.monitor_id) {
                manager.restart(
                    config.monitor_id.clone(),
                    db.inner().clone(),
                    app_handle.clone(),
                );
            }
        }
    }

    Ok(())
}