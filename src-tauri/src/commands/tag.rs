use tauri::State;

use crate::ctx::AppContext;
use crate::dto::tag_dto::{
    DeleteTagRequest, GetWallpaperTagsRequest, RenameTagRequest, SetWallpaperTagsRequest,
    TagWallpapersRequest, UntagWallpapersRequest,
};
use crate::dto::Validated;
use crate::entities::tag;
use crate::services::tag_service::{self, TagWithCount};

use super::error::CommandResult;

/// 获取全部标签（带引用计数）
#[tauri::command]
pub async fn get_tags(ctx: State<'_, AppContext>) -> CommandResult<Vec<TagWithCount>> {
    Ok(tag_service::get_all_with_count(&ctx.db).await?)
}

/// 获取某壁纸的标签列表
#[tauri::command]
pub async fn get_wallpaper_tags(
    ctx: State<'_, AppContext>,
    req: Validated<GetWallpaperTagsRequest>,
) -> CommandResult<Vec<tag::Model>> {
    let req = req.into_inner();
    Ok(tag_service::get_wallpaper_tags(&ctx.db, req.wallpaper_id).await?)
}

/// 给一批壁纸打一批标签（resolve-or-create + 幂等）。返回新增关联条数。
#[tauri::command]
pub async fn tag_wallpapers(
    ctx: State<'_, AppContext>,
    req: Validated<TagWallpapersRequest>,
) -> CommandResult<u64> {
    let req = req.into_inner();
    Ok(tag_service::tag_wallpapers(&ctx.db, req.wallpaper_ids, req.tag_names).await?)
}

/// 从一批壁纸移除一批标签（按 tag id）。返回删除关联条数。
#[tauri::command]
pub async fn untag_wallpapers(
    ctx: State<'_, AppContext>,
    req: Validated<UntagWallpapersRequest>,
) -> CommandResult<u64> {
    let req = req.into_inner();
    Ok(tag_service::untag_wallpapers(&ctx.db, req.wallpaper_ids, req.tag_ids).await?)
}

/// 覆盖式设置单张壁纸的标签集合。返回设置后的完整标签列表。
#[tauri::command]
pub async fn set_wallpaper_tags(
    ctx: State<'_, AppContext>,
    req: Validated<SetWallpaperTagsRequest>,
) -> CommandResult<Vec<tag::Model>> {
    let req = req.into_inner();
    Ok(tag_service::set_wallpaper_tags(&ctx.db, req.wallpaper_id, req.tag_names).await?)
}

/// 重命名标签
#[tauri::command]
pub async fn rename_tag(
    ctx: State<'_, AppContext>,
    req: Validated<RenameTagRequest>,
) -> CommandResult<tag::Model> {
    let req = req.into_inner();
    Ok(tag_service::rename_tag(&ctx.db, req.id, req.name).await?)
}

/// 删除标签（连带清理关联）
#[tauri::command]
pub async fn delete_tag(
    ctx: State<'_, AppContext>,
    req: Validated<DeleteTagRequest>,
) -> CommandResult<()> {
    let req = req.into_inner();
    Ok(tag_service::delete_tag(&ctx.db, req.id).await?)
}
