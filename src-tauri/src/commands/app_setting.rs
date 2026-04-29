use std::collections::HashMap;
use std::sync::Arc;

use tauri::State;
use tokio::sync::Mutex;

use crate::ctx::AppContext;
use crate::runtime::Scheduler;
use crate::dto::app_setting_dto::{GetSettingRequest, SetSettingRequest};
use crate::dto::Validated;
use crate::services::app_setting_service;

use super::error::CommandResult;

/// 获取所有设置（返回 key-value 对象）
#[tauri::command]
pub async fn get_settings(
    ctx: State<'_, AppContext>,
) -> CommandResult<HashMap<String, String>> {
    let settings = app_setting_service::get_all(&ctx.db).await?;

    let map: HashMap<String, String> = settings
        .into_iter()
        .map(|s| (s.key, s.value))
        .collect();

    Ok(map)
}

/// 获取单个设置值
#[tauri::command]
pub async fn get_setting(
    ctx: State<'_, AppContext>,
    req: Validated<GetSettingRequest>,
) -> CommandResult<Option<String>> {
    let req = req.into_inner();
    Ok(app_setting_service::get(&ctx.db, &req.key).await?)
}

/// 设置键值对（写入 DB + 按 key 触发副作用）
///
/// 统一入口：前端所有 setting 变更都通过此 command，
/// 内部通过 Scheduler 的副作用方法，在写入 DB 后立即执行对应的副作用，
/// 确保设置变更即时生效。
///
/// `monitor_id`: 可选参数，display_mode 变更时需要传入当前选中的显示器 ID，
/// 用于确定"基准显示器"以同步配置到其他显示器。
#[tauri::command]
pub async fn set_setting(
    ctx: State<'_, AppContext>,
    scheduler: State<'_, Arc<Mutex<Scheduler>>>,
    req: Validated<SetSettingRequest>,
    monitor_id: Option<String>,
) -> CommandResult<()> {
    let req = req.into_inner();

    // 跨字段校验：按 key 校验 value 格式
    req.validate_value_format()?;

    // 1. 写入 DB
    app_setting_service::set(&ctx.db, &req.key, &req.value).await?;

    // 2. 通过 Scheduler 执行副作用
    let mut sched = scheduler.lock().await;
    sched.apply_setting_side_effect(&req.key, &req.value, monitor_id.as_deref()).await;

    Ok(())
}
