use std::sync::Arc;

use tauri::State;
use tokio::sync::Mutex;

use crate::ctx::AppContext;
use crate::runtime::carousel::{
    carousel_key, collection_has_enough_wallpapers, should_start_timer, CarouselTask,
};
use crate::runtime::Scheduler;
use crate::dto::monitor_config_dto::{
    DeleteMonitorConfigRequest, GetMonitorConfigRequest, UpsertMonitorConfigRequest,
};
use crate::dto::Validated;
use crate::entities::monitor_config;
use crate::services::monitor_config_service;

use log::info;

/// 获取所有显示器配置
#[tauri::command]
pub async fn get_monitor_configs(
    ctx: State<'_, AppContext>,
) -> Result<Vec<monitor_config::Model>, String> {
    monitor_config_service::get_all(&ctx.db)
        .await
        .map_err(|e| e.to_string())
}

/// 根据 monitor_id 获取配置
#[tauri::command]
pub async fn get_monitor_config(
    ctx: State<'_, AppContext>,
    req: Validated<GetMonitorConfigRequest>,
) -> Result<Option<monitor_config::Model>, String> {
    let req = req.into_inner();
    monitor_config_service::get_by_monitor_id(&ctx.db, &req.monitor_id)
        .await
        .map_err(|e| e.to_string())
}

/// 创建或更新显示器配置
///
/// upsert 后自动管理定时器：
/// - 满足轮播条件 → 通过 Scheduler 启动/重启定时器
/// - 不满足 → 通过 Scheduler 停止定时器
#[tauri::command]
pub async fn upsert_monitor_config(
    ctx: State<'_, AppContext>,
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
    req: Validated<UpsertMonitorConfigRequest>,
) -> Result<monitor_config::Model, String> {
    let req = req.into_inner();

    // 跨字段校验
    req.validate_cross_fields()?;

    let config = monitor_config_service::upsert(
        &ctx.db,
        &req.monitor_id,
        req.wallpaper_id,
        req.collection_id,
        req.clear_collection,
        req.display_mode.as_deref(),
        req.fit_mode.as_deref(),
        req.play_mode.as_deref(),
        req.play_interval,
        req.is_enabled,
        req.active,
    )
    .await
    .map_err(|e| e.to_string())?;

    // ===== 定时器管理 =====
    manage_timer_for_config(&ctx, &scheduler, &config).await;

    Ok(config)
}

/// 根据 config 状态管理定时器
async fn manage_timer_for_config(
    ctx: &AppContext,
    scheduler: &Arc<Mutex<Scheduler>>,
    config: &monitor_config::Model,
) {
    let key = carousel_key(&config.monitor_id);
    let mut sched = scheduler.lock().await;

    if should_start_timer(config) {
        let cid = config.collection_id.unwrap(); // safe: should_start_timer 已检查

        // 检查收藏夹壁纸数 > 1
        match collection_has_enough_wallpapers(&ctx.db, cid).await {
            Ok(true) => {
                sched.restart(
                    key,
                    CarouselTask {
                        app: ctx.app_handle.clone(),
                        monitor_id: config.monitor_id.clone(),
                    },
                );
            }
            Ok(false) => {
                sched.stop(&key);
            }
            Err(e) => {
                log::warn!("[upsert] Failed to check collection wallpapers: {}", e);
                sched.stop(&key);
            }
        }
    } else {
        sched.stop(&key);
    }
}

/// 删除显示器配置
#[tauri::command]
pub async fn delete_monitor_config(
    ctx: State<'_, AppContext>,
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
    req: Validated<DeleteMonitorConfigRequest>,
) -> Result<(), String> {
    let req = req.into_inner();

    // 先停止可能存在的定时器
    if let Some(mid) = &req.monitor_id {
        let mut sched = scheduler.lock().await;
        sched.stop(&carousel_key(mid));
    }

    monitor_config_service::delete(&ctx.db, req.id)
        .await
        .map_err(|e| e.to_string())
}

/// 启动所有满足轮播条件的定时器（应用启动时由前端调用）
#[tauri::command]
pub async fn start_timers(
    ctx: State<'_, AppContext>,
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
) -> Result<(), String> {
    let configs = monitor_config_service::get_all(&ctx.db)
        .await
        .map_err(|e| e.to_string())?;

    let mut sched = scheduler.lock().await;

    for config in &configs {
        if should_start_timer(config) {
            let cid = config.collection_id.unwrap();
            match collection_has_enough_wallpapers(&ctx.db, cid).await {
                Ok(true) => {
                    let key = carousel_key(&config.monitor_id);
                    sched.spawn(
                        key.clone(),
                        CarouselTask {
                            app: ctx.app_handle.clone(),
                            monitor_id: config.monitor_id.clone(),
                        },
                    );
                    info!("[start_timers] 启动定时器: {}", config.monitor_id);
                }
                Ok(false) => {}
                Err(e) => {
                    log::warn!(
                        "[start_timers] 检查收藏夹失败 {}: {}",
                        config.monitor_id,
                        e
                    );
                }
            }
        }
    }

    Ok(())
}