use std::sync::Arc;

use tauri::State;
use tokio::sync::Mutex;

use crate::application::{self, AppServices};
use crate::context::AppContext;
use crate::dto::scene_dto::{
    ApplySceneResult, RenameSceneRequest, SaveSceneRequest, SceneIdRequest, SceneState,
    SceneSummary,
};
use crate::dto::Validated;
use crate::tasks::registry::TaskRegistry;

use super::error::CommandResult;

/// 当前桌面状态（与快照 payload 同构，供弹窗阅览确认）
#[tauri::command]
pub async fn get_current_scene_state(ctx: State<'_, Arc<AppContext>>) -> CommandResult<SceneState> {
    Ok(application::scene::current_state(&ctx.inner().db).await?)
}

#[tauri::command]
pub async fn list_scenes(
    ctx: State<'_, Arc<AppContext>>,
    scheduler: State<'_, Arc<Mutex<TaskRegistry>>>,
) -> CommandResult<Vec<SceneSummary>> {
    let svc = AppServices::new(ctx.inner(), scheduler.inner());
    Ok(application::scene::list_scenes(&svc).await?)
}

#[tauri::command]
pub async fn save_scene(
    ctx: State<'_, Arc<AppContext>>,
    scheduler: State<'_, Arc<Mutex<TaskRegistry>>>,
    req: Validated<SaveSceneRequest>,
) -> CommandResult<SceneSummary> {
    let svc = AppServices::new(ctx.inner(), scheduler.inner());
    Ok(application::scene::save_scene(&svc, &req.into_inner().name).await?)
}

#[tauri::command]
pub async fn rename_scene(
    ctx: State<'_, Arc<AppContext>>,
    scheduler: State<'_, Arc<Mutex<TaskRegistry>>>,
    req: Validated<RenameSceneRequest>,
) -> CommandResult<SceneSummary> {
    let svc = AppServices::new(ctx.inner(), scheduler.inner());
    let req = req.into_inner();
    Ok(application::scene::rename_scene(&svc, req.id, &req.name).await?)
}

#[tauri::command]
pub async fn delete_scene(
    ctx: State<'_, Arc<AppContext>>,
    scheduler: State<'_, Arc<Mutex<TaskRegistry>>>,
    req: Validated<SceneIdRequest>,
) -> CommandResult<()> {
    let svc = AppServices::new(ctx.inner(), scheduler.inner());
    Ok(application::scene::delete_scene(&svc, req.into_inner().id).await?)
}

#[tauri::command]
pub async fn apply_scene(
    ctx: State<'_, Arc<AppContext>>,
    scheduler: State<'_, Arc<Mutex<TaskRegistry>>>,
    req: Validated<SceneIdRequest>,
) -> CommandResult<ApplySceneResult> {
    let svc = AppServices::new(ctx.inner(), scheduler.inner());
    Ok(application::scene::apply_scene(&svc, req.into_inner().id).await?)
}
