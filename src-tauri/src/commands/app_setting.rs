use std::collections::HashMap;
use std::sync::Arc;

use tauri::State;
use tokio::sync::Mutex;

use crate::application::{self, AppServices};
use crate::context::AppContext;
use crate::dto::app_setting_dto::{GetSettingRequest, SetSettingRequest};
use crate::dto::Validated;
use crate::services::app_setting_service;
use crate::tasks::registry::TaskRegistry;

use super::error::CommandResult;

/// 获取所有设置（返回 key-value 对象）
#[tauri::command]
pub async fn get_settings(
    ctx: State<'_, Arc<AppContext>>,
) -> CommandResult<HashMap<String, String>> {
    let app_ctx: &AppContext = ctx.inner();
    let settings = app_setting_service::get_all(&app_ctx.db).await?;

    let map: HashMap<String, String> = settings.into_iter().map(|s| (s.key, s.value)).collect();

    Ok(map)
}

/// 获取单个设置值
#[tauri::command]
pub async fn get_setting(
    ctx: State<'_, Arc<AppContext>>,
    req: Validated<GetSettingRequest>,
) -> CommandResult<Option<String>> {
    let req = req.into_inner();
    let app_ctx: &AppContext = ctx.inner();
    Ok(app_setting_service::get(&app_ctx.db, &req.key).await?)
}

/// 设置键值对（写入 DB + 按 key 触发副作用）
///
/// 统一入口：前端所有 setting 变更都通过此 command，
/// 内部通过 TaskRegistry 的副作用方法，在写入 DB 后立即执行对应的副作用，
/// 确保设置变更即时生效。
///
/// `monitor_id`: 可选参数，display_mode 变更时需要传入当前选中的显示器 ID，
/// 用于确定"基准显示器"以同步配置到其他显示器。
#[tauri::command]
pub async fn set_setting(
    ctx: State<'_, Arc<AppContext>>,
    scheduler: State<'_, Arc<Mutex<TaskRegistry>>>,
    req: Validated<SetSettingRequest>,
) -> CommandResult<()> {
    let svc = AppServices::new(ctx.inner(), scheduler.inner());
    Ok(application::settings::set_setting(&svc, &req.into_inner()).await?)
}
