//! 场景快照用例：把「当前桌面状态」变成一个可命名、可来回切换的状态书签。
//!
//! ## 恢复路径为什么这样写
//!
//! 写库绝不能直写后自己刷新：`setting_effects::apply_display_mode_side_effect`
//! 承担同步组配置对齐、一次 reconcile(All)、轮播 timer 重建与牌堆清空，
//! 绕过它就会出现「DB 变了但 renderer / timer 没跟上」。这里在事务写完后
//! 复用该编排；independent 分支不会覆盖逐屏配置，mirror/extend 分支用基准屏
//! 覆盖整组——与快照保存时同步组本来就一致，语义不冲突。
//!
//! 前端刷新用事件而非 invoke 信号：托盘点击场景不经过前端 invoke，
//! 事件是唯一能同时覆盖两条入口的通道。

use std::collections::HashSet;

use anyhow::{bail, Context, Result};
use log::warn;
use sea_orm::prelude::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    Set, TransactionTrait,
};

use super::{effects::AppEffects, AppServices};
use crate::dto::app_setting_dto::keys as setting_keys;
use crate::dto::scene_dto::{ApplySceneResult, SceneMonitorState, SceneState, SceneSummary};
use crate::entities::{app_setting, collection, monitor_config, scene_snapshot, wallpaper};
use crate::events::{AppSettingsRefreshedPayload, MonitorConfigRefreshedPayload, TypedEmit};
use crate::services::{
    app_setting_service, monitor_config_service, scene_service, wallpaper_service,
};

const DEFAULT_DISPLAY_MODE: &str = "independent";
const DEFAULT_VOLUME: &str = "50";

/// 读取当前桌面状态：只捕 active 屏的业务字段与两个全局设置。
/// 与存进 payload_json 的结构完全同构，弹窗里看到什么就保存什么。
pub async fn current_state(db: &DatabaseConnection) -> Result<SceneState> {
    let monitors: Vec<SceneMonitorState> = monitor_config_service::get_all(db)
        .await?
        .into_iter()
        .filter(|c| c.active)
        .map(|c| SceneMonitorState {
            monitor_id: c.monitor_id,
            wallpaper_id: c.wallpaper_id,
            collection_id: c.collection_id,
            fit_mode: c.fit_mode,
            play_mode: c.play_mode,
            play_interval: c.play_interval,
            is_enabled: c.is_enabled,
        })
        .collect();

    Ok(SceneState {
        display_mode: setting_or(db, setting_keys::DISPLAY_MODE, DEFAULT_DISPLAY_MODE).await?,
        global_volume: setting_or(db, setting_keys::GLOBAL_VOLUME, DEFAULT_VOLUME).await?,
        monitors,
    })
}

async fn setting_or(db: &DatabaseConnection, key: &str, default: &str) -> Result<String> {
    Ok(app_setting_service::get(db, key)
        .await?
        .unwrap_or_else(|| default.to_string()))
}

fn to_summary(model: scene_snapshot::Model, state: SceneState) -> SceneSummary {
    SceneSummary {
        id: model.id,
        name: model.name,
        created_at: model.created_at,
        updated_at: model.updated_at,
        state,
    }
}

/// 纯函数：把表行转成可渲染摘要，托盘与命令共用同一份解析口径。
pub fn list_scenes_from_db(rows: Vec<scene_snapshot::Model>) -> Vec<SceneSummary> {
    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        match serde_json::from_str::<SceneState>(&row.payload_json) {
            Ok(state) => result.push(to_summary(row, state)),
            // 一条坏数据不该让整个列表打不开。
            Err(e) => warn!("[Scene] 快照 {} payload 损坏，跳过: {}", row.id, e),
        }
    }
    result
}

pub async fn list_scenes(svc: &AppServices<'_>) -> Result<Vec<SceneSummary>> {
    Ok(list_scenes_from_db(
        scene_service::get_all(&svc.ctx.db).await?,
    ))
}

/// 场景增删改 / 应用后刷新托盘菜单。
/// 数据在 async 上下文里准备好，托盘函数本身只做同步的菜单构造 + set_menu，
/// 避免在 tokio 线程里嵌套 block_on。
async fn refresh_scene_menu(svc: &AppServices<'_>) {
    let rows = scene_service::get_all(&svc.ctx.db)
        .await
        .unwrap_or_default();
    let summaries = list_scenes_from_db(rows);
    crate::platform::tray::refresh_tray_menu(
        &svc.ctx.app_handle,
        &summaries,
        svc.ctx.last_scene_id(),
    );
}

/// 保存当前状态；同名覆盖，并返回可立即渲染的摘要。
pub async fn save_scene(svc: &AppServices<'_>, name: &str) -> Result<SceneSummary> {
    let name = name.trim();
    if name.is_empty() {
        bail!("场景名称不能为空");
    }
    let state = current_state(&svc.ctx.db).await?;
    let payload = serde_json::to_string(&state).context("Failed to serialize scene state")?;
    let model = scene_service::save(&svc.ctx.db, name, &payload).await?;
    refresh_scene_menu(svc).await;
    Ok(to_summary(model, state))
}

pub async fn rename_scene(svc: &AppServices<'_>, id: i32, name: &str) -> Result<SceneSummary> {
    let name = name.trim();
    if name.is_empty() {
        bail!("场景名称不能为空");
    }
    let model = scene_service::rename(&svc.ctx.db, id, name).await?;
    let state = serde_json::from_str::<SceneState>(&model.payload_json)
        .context("Failed to parse scene state")?;
    refresh_scene_menu(svc).await;
    Ok(to_summary(model, state))
}

pub async fn delete_scene(svc: &AppServices<'_>, id: i32) -> Result<()> {
    scene_service::delete(&svc.ctx.db, id).await?;
    // 回切目标失效时清掉，避免指向已删场景。
    if svc.ctx.last_scene_id() == Some(id) {
        svc.ctx.set_last_scene_id(None);
    }
    refresh_scene_menu(svc).await;
    Ok(())
}

/// 校验快照里引用的壁纸仍可显示（未删除 / 未进回收站）。
async fn wallpaper_settable(db: &DatabaseConnection, id: i32) -> Result<bool> {
    Ok(wallpaper::Entity::find_by_id(id)
        .filter(wallpaper_service::not_deleted())
        .one(db)
        .await?
        .is_some())
}

/// 在事务内写 app_settings：update 优先，不存在则 insert。
async fn set_setting_in_txn<C: ConnectionTrait>(txn: &C, key: &str, value: &str) -> Result<()> {
    let result = app_setting::Entity::update_many()
        .col_expr(app_setting::Column::Value, Expr::value(value))
        .filter(app_setting::Column::Key.eq(key))
        .exec(txn)
        .await?;
    if result.rows_affected == 0 {
        let active = app_setting::ActiveModel {
            key: Set(key.to_string()),
            value: Set(value.to_string()),
        };
        active.insert(txn).await?;
    }
    Ok(())
}

/// 应用场景：严格按 monitor_id 对齐，失效项跳过并汇总提示，不做兜底。
pub async fn apply_scene(svc: &AppServices<'_>, id: i32) -> Result<ApplySceneResult> {
    let db = &svc.ctx.db;
    let model = scene_service::find_by_id(db, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("场景不存在"))?;
    let state: SceneState =
        serde_json::from_str(&model.payload_json).context("场景数据损坏，无法应用")?;

    let active_ids: HashSet<String> = monitor_config_service::get_all(db)
        .await?
        .into_iter()
        .filter(|c| c.active)
        .map(|c| c.monitor_id)
        .collect();

    let mut skipped = Vec::new();
    let mut plans: Vec<SceneMonitorState> = Vec::new();
    for monitor in &state.monitors {
        if !active_ids.contains(&monitor.monitor_id) {
            skipped.push(format!("{}：显示器已变化", monitor.monitor_id));
            continue;
        }
        if let Some(wid) = monitor.wallpaper_id {
            if !wallpaper_settable(db, wid).await? {
                skipped.push(format!("{}：引用的壁纸已失效", monitor.monitor_id));
                continue;
            }
        }
        if let Some(cid) = monitor.collection_id {
            if collection::Entity::find_by_id(cid).one(db).await?.is_none() {
                skipped.push(format!("{}：引用的收藏夹已删除", monitor.monitor_id));
                continue;
            }
        }
        plans.push(monitor.clone());
    }

    // 新增的显示器不在快照里：明确提示布局变化，保持它原样，不静默贴错配置。
    let snapshot_ids: HashSet<&str> = state
        .monitors
        .iter()
        .map(|m| m.monitor_id.as_str())
        .collect();
    if active_ids
        .iter()
        .any(|id| !snapshot_ids.contains(id.as_str()))
    {
        skipped.push("显示器布局已变化，新增屏幕保持原样".to_string());
    }

    // 单事务写全组配置 + 两个全局设置，避免 watcher 看到中间态。
    let txn = db.begin().await?;
    for monitor in &plans {
        monitor_config::Entity::update_many()
            .col_expr(
                monitor_config::Column::WallpaperId,
                Expr::value(monitor.wallpaper_id),
            )
            .col_expr(
                monitor_config::Column::CollectionId,
                Expr::value(monitor.collection_id),
            )
            .col_expr(
                monitor_config::Column::FitMode,
                Expr::value(monitor.fit_mode.clone()),
            )
            .col_expr(
                monitor_config::Column::PlayMode,
                Expr::value(monitor.play_mode.clone()),
            )
            .col_expr(
                monitor_config::Column::PlayInterval,
                Expr::value(monitor.play_interval),
            )
            .col_expr(
                monitor_config::Column::IsEnabled,
                Expr::value(monitor.is_enabled),
            )
            .filter(monitor_config::Column::MonitorId.eq(monitor.monitor_id.clone()))
            .exec(&txn)
            .await?;
    }
    set_setting_in_txn(&txn, setting_keys::DISPLAY_MODE, &state.display_mode).await?;
    set_setting_in_txn(&txn, setting_keys::GLOBAL_VOLUME, &state.global_volume).await?;
    txn.commit().await?;

    // 复用既有副作用编排：同步组对齐 + 一次 reconcile(All) + timer 重建 + 清牌堆。
    {
        let mut guard = svc.scheduler.lock().await;
        let mut fx = AppEffects::new(svc.ctx, &mut guard);
        fx.apply_setting_side_effect(setting_keys::DISPLAY_MODE, &state.display_mode, None)
            .await;
    }

    let app = &svc.ctx.app_handle;
    if let Err(e) = app.typed_emit(&MonitorConfigRefreshedPayload) {
        warn!("[Scene] 发送 monitor-config-refreshed 失败: {}", e);
    }
    if let Err(e) = app.typed_emit(&AppSettingsRefreshedPayload) {
        warn!("[Scene] 发送 app-settings-refreshed 失败: {}", e);
    }

    svc.ctx.set_last_scene_id(Some(id));
    refresh_scene_menu(svc).await;

    Ok(ApplySceneResult {
        applied: plans.len(),
        skipped,
    })
}
