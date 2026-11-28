//! 目录监听服务（P2 同步核心 / P3 触发源 / P4 生命周期）。
//!
//! ## 设计原则
//!
//! - **严格同步**：监听收藏夹的成员 == 目录下合规文件。文件消失即硬删记录，
//!   不存在「软删除 / missing 状态」；目录失效则连收藏夹一起清空，
//!   只保留 `watched_folders` 里的一条 `valid = 0` 记录等用户重选。
//! - **与原链路隔离**：wallpapers 同表但 `is_external = 1`，本地查询统一走
//!   `wallpaper_service::local_only()`；删除不走 `trash_batch`（它有 remove_file）。
//! - **单一入口**：进入夹 / 手动刷新 / 后台轮询都调 `sync_folder`，
//!   靠 per-folder 互斥 + `viewRequestSeq`（前端）避免并发与串台。
//! - **失效清理复用 `purge_watched_folder`**：P4 目录失效与 P6 删除链路共用，
//!   事务内按 id 清 monitor 引用 / 关联 / 行，事务外删缩略图、reconcile、emit。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use sea_orm::{
    sea_query::Expr, ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, Set, TransactionTrait,
};
use serde::Serialize;
use tauri::{Emitter, Manager};

use crate::context::unified_manager::ReconcileScope;
use crate::context::AppContext;
use crate::entities::{
    collection, collection_wallpaper, monitor_config, wallpaper, wallpaper_tag, watched_folder,
};
use crate::services::{app_setting_service, wallpaper_service};
use crate::utils::time::now_str;

/// 目录失效时上抛给前端的可识别错误（toast + activeId=0 + 刷新 collections）。
pub const ERR_PATH_INVALID: &str = "WATCHED_PATH_INVALID";

/// 轮询周期配置键（分钟），缺省 5，范围 1..=60。
pub const SETTING_SCAN_INTERVAL_MIN: &str = "watched_scan_interval_min";
pub const DEFAULT_SCAN_INTERVAL_MIN: u64 = 5;

/// 监听目录变化事件：前端据此刷新 collections / 强制 activeId=0。
pub const EVENT_WATCHED_CHANGED: &str = "watched-folders-changed";

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncReport {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    /// 本次新增的视频 id，前端进夹/刷新时据此补抽帧（后台扫描不抽帧）
    pub new_video_ids: Vec<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WatchedFolderView {
    pub id: i32,
    pub path: String,
    pub name: String,
    pub collection_id: Option<i32>,
    pub valid: i32,
    pub last_scan_at: Option<String>,
}

impl From<watched_folder::Model> for WatchedFolderView {
    fn from(m: watched_folder::Model) -> Self {
        Self {
            id: m.id,
            path: m.path,
            name: m.name,
            collection_id: m.collection_id,
            valid: m.valid,
            last_scan_at: m.last_scan_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AddWatchedFolderResult {
    pub folder_id: i32,
    pub collection_id: i32,
    /// 目录名，前端据此弹「手动更名」提示
    pub collection_name: String,
    pub report: SyncReport,
}

// ==================== 路径与 asset 权限 ====================

/// opener 选中的路径统一 canonicalize 后作为唯一键，与 Tauri scope 的
/// `is_allowed`（内部也会 canonicalize）保持同一口径。
pub fn canonicalize_dir(path: &str) -> Result<String> {
    let canonical = Path::new(path)
        .canonicalize()
        .with_context(|| format!("无法解析监听目录: {path}"))?;
    if !canonical.is_dir() {
        bail!("监听路径不是目录: {path}");
    }
    Ok(canonical.to_string_lossy().to_string())
}

/// 授权 WebView 读取该目录（asset://）。scope 是进程内存态，重启必须重放。
pub fn grant_asset_scope(app: &tauri::AppHandle, path: &str) -> Result<()> {
    let scope = app.asset_protocol_scope();
    scope
        .allow_directory(path, true)
        .with_context(|| format!("授权 asset scope 失败: {path}"))?;
    // 自检：allow 成功不代表 pattern 真的能匹配（路径大小写/短名差异）。
    if !scope.is_allowed(path) {
        bail!("asset scope 自检失败: {path}");
    }
    Ok(())
}

/// 启动编排调用：把 DB 中所有有效监听目录重新授权，并逐条自检。
pub async fn restore_asset_scopes(app: &tauri::AppHandle, db: &DatabaseConnection) -> Result<()> {
    let paths = watched_folder::Entity::find()
        .filter(watched_folder::Column::Valid.eq(1))
        .all(db)
        .await?;
    for folder in paths {
        if let Err(e) = grant_asset_scope(app, &folder.path) {
            // 单个目录失败不该阻断启动：失效目录会在存活探测里被标记。
            log::warn!("[Watched] 恢复 asset scope 失败 {}: {e:#}", folder.path);
        }
    }
    Ok(())
}

// ==================== 查询 ====================

pub async fn list(db: &DatabaseConnection) -> Result<Vec<WatchedFolderView>> {
    let rows = watched_folder::Entity::find()
        .order_by_asc(watched_folder::Column::Id)
        .all(db)
        .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

async fn find_by_collection(
    db: &DatabaseConnection,
    collection_id: i32,
) -> Result<Option<watched_folder::Model>> {
    Ok(watched_folder::Entity::find()
        .filter(watched_folder::Column::CollectionId.eq(collection_id))
        .one(db)
        .await?)
}

// ==================== 生命周期（P4） ====================

/// 新建（或重选失效路径后重新激活）一个监听目录。
///
/// `path` 为 unique key：有效状态下重复选择直接拒绝；失效状态下复用同一行、
/// 重新建夹、重新开启扫描。路径失效记录本身从不自动删除。
pub async fn create_or_reactivate(ctx: &AppContext, path: &str) -> Result<AddWatchedFolderResult> {
    let canonical = canonicalize_dir(path)?;
    // 授权放在任何写库之前：scope 失败时不能留下「夹已建、folder 行 valid=1
    // 但前端报错」的半截状态。移除监听也不再 forbid（见 remove_folder），
    // 因此同一路径重新添加不会撞上 forbidden_patterns 残留。
    grant_asset_scope(&ctx.app_handle, &canonical)?;
    let db = &ctx.db;

    let existing = watched_folder::Entity::find()
        .filter(watched_folder::Column::Path.eq(canonical.clone()))
        .one(db)
        .await?;

    if let Some(folder) = &existing {
        if folder.valid == 1 {
            bail!("该目录已在监听中");
        }
    }

    let name = Path::new(&canonical)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Watched")
        .to_string();

    // 建夹：kind = watched，只在 watched 组内分配连续 sort_order
    let collection = create_watched_collection(db, &name).await?;

    let folder = match existing {
        Some(model) => {
            let mut active: watched_folder::ActiveModel = model.into();
            active.collection_id = Set(Some(collection.id));
            active.valid = Set(1);
            active.last_scan_at = Set(None);
            active.update(db).await?
        }
        None => {
            watched_folder::ActiveModel {
                path: Set(canonical.clone()),
                name: Set(name.clone()),
                collection_id: Set(Some(collection.id)),
                valid: Set(1),
                ..Default::default()
            }
            .insert(db)
            .await?
        }
    };

    let report = sync_folder(ctx, folder.id, true).await?;
    emit_changed(&ctx.app_handle, None);

    Ok(AddWatchedFolderResult {
        folder_id: folder.id,
        collection_id: collection.id,
        collection_name: name,
        report,
    })
}

/// 只在 watched 组内追加，避免与 manual+smart 组的 sort_order 空间冲突。
pub async fn create_watched_collection(
    db: &DatabaseConnection,
    name: &str,
) -> Result<collection::Model> {
    let max = collection::Entity::find()
        .filter(collection::Column::Kind.eq("watched"))
        .order_by_desc(collection::Column::SortOrder)
        .one(db)
        .await?
        .map(|m| m.sort_order)
        .unwrap_or(crate::services::collection_service::WATCHED_SORT_BASE - 1);

    Ok(collection::ActiveModel {
        name: Set(name.to_string()),
        sort_order: Set(max + 1),
        is_builtin: Set(0),
        kind: Set("watched".to_string()),
        ..Default::default()
    }
    .insert(db)
    .await?)
}

/// 用户手动移除监听：清库 + 删夹 + 删记录 + 撤销授权（不碰源文件）。
pub async fn remove_folder(ctx: &AppContext, folder_id: i32) -> Result<()> {
    let folder = watched_folder::Entity::find_by_id(folder_id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| anyhow!("监听目录不存在: {folder_id}"))?;

    purge_watched_folder(ctx, &folder, true).await?;
    // 刻意不调用 forbid_directory：Tauri 的 forbidden_patterns 优先于
    // allowed_patterns，且 allow_directory 不会把它清掉。移除后重选同一路径
    // 会让 is_allowed 自检永久失败（重启才恢复，因为 scope 是内存态）。
    // DB 记录已删，启动重放只会授权有效路径，无需显式撤销。
    emit_changed(&ctx.app_handle, folder.collection_id);
    Ok(())
}

/// 路径失效：清空该夹全部 wallpaper 记录与缩略图、删除收藏夹，
/// 只保留 `valid = 0` 的监听路径行。恢复唯一入口是 opener 重选。
pub async fn invalidate_folder(ctx: &AppContext, folder_id: i32) -> Result<()> {
    let folder = watched_folder::Entity::find_by_id(folder_id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| anyhow!("监听目录不存在: {folder_id}"))?;
    purge_watched_folder(ctx, &folder, false).await?;
    emit_changed(&ctx.app_handle, folder.collection_id);
    Ok(())
}

/// P4 / P6 共用的批量清理：事务内按 id 精确清引用与记录，事务外删文件、
/// reconcile(All) 派发全部屏、emit 通知前端（activeId=0 + 刷新 collections）。
pub async fn purge_watched_folder(
    ctx: &AppContext,
    folder: &watched_folder::Model,
    delete_folder_row: bool,
) -> Result<()> {
    let db = &ctx.db;
    let txn = db.begin().await?;

    let wallpapers = wallpaper::Entity::find()
        .filter(wallpaper::Column::WatchedFolderId.eq(folder.id))
        .all(&txn)
        .await?;
    let ids: Vec<i32> = wallpapers.iter().map(|w| w.id).collect();
    let thumb_paths: Vec<PathBuf> = wallpapers
        .iter()
        .filter_map(|w| w.thumb_path.as_ref().map(PathBuf::from))
        .collect();

    if !ids.is_empty() {
        monitor_config::Entity::update_many()
            .col_expr(
                monitor_config::Column::WallpaperId,
                Expr::value(sea_orm::Value::Int(None)),
            )
            .filter(monitor_config::Column::WallpaperId.is_in(ids.clone()))
            .exec(&txn)
            .await?;

        collection_wallpaper::Entity::delete_many()
            .filter(collection_wallpaper::Column::WallpaperId.is_in(ids.clone()))
            .exec(&txn)
            .await?;

        wallpaper_tag::Entity::delete_many()
            .filter(wallpaper_tag::Column::WallpaperId.is_in(ids.clone()))
            .exec(&txn)
            .await?;

        wallpaper::Entity::delete_many()
            .filter(wallpaper::Column::Id.is_in(ids.clone()))
            .exec(&txn)
            .await?;
    }

    if let Some(cid) = folder.collection_id {
        monitor_config::Entity::update_many()
            .col_expr(
                monitor_config::Column::CollectionId,
                Expr::value(sea_orm::Value::Int(None)),
            )
            .filter(monitor_config::Column::CollectionId.eq(cid))
            .exec(&txn)
            .await?;
        collection_wallpaper::Entity::delete_many()
            .filter(collection_wallpaper::Column::CollectionId.eq(cid))
            .exec(&txn)
            .await?;
        collection::Entity::delete_by_id(cid).exec(&txn).await?;
    }

    if delete_folder_row {
        watched_folder::Entity::delete_by_id(folder.id)
            .exec(&txn)
            .await?;
    } else {
        let mut active: watched_folder::ActiveModel = folder.clone().into();
        active.collection_id = Set(None);
        active.valid = Set(0);
        active.last_scan_at = Set(None);
        active.update(&txn).await?;
    }

    txn.commit().await?;

    for path in thumb_paths {
        if let Err(e) = std::fs::remove_file(&path) {
            log::warn!("[Watched] 清理缩略图失败 {:?}: {}", path, e);
        }
    }

    // 引用清空后必须让桌面立刻切走，不能只改 DB。
    let mut guard = ctx.unified_manager.lock().await;
    if let Err(e) = guard.reconcile(ReconcileScope::All).await {
        log::warn!("[Watched] 失效清理 reconcile 失败: {e}");
    }
    guard.notify_config_refreshed();
    drop(guard);

    Ok(())
}

/// `invalidated_collection_id` 非空表示这个夹刚被删（失效/移除），
/// 前端据此在 activeId 正指向它时强制回本地壁纸栏，避免指向不存在的夹。
#[derive(Clone, Serialize)]
struct WatchedChangedPayload {
    invalidated_collection_id: Option<i32>,
}

fn emit_changed(app: &tauri::AppHandle, invalidated_collection_id: Option<i32>) {
    if let Err(e) = app.emit(
        EVENT_WATCHED_CHANGED,
        WatchedChangedPayload {
            invalidated_collection_id,
        },
    ) {
        log::warn!("[Watched] emit 失败: {e}");
    }
}

// ==================== 同步核心（P2） ====================

/// per-folder 互斥守卫：临界区零 await，用同步锁 + RAII 引用即可，
/// 不同目录之间仍可并行，只有同一目录的重复触发会被跳过。
struct SyncGuard<'a> {
    set: &'a std::sync::Mutex<HashSet<i32>>,
    id: i32,
}

impl<'a> SyncGuard<'a> {
    fn acquire(ctx: &'a AppContext, id: i32) -> Option<Self> {
        let mut set = ctx.watched_syncing.lock().unwrap();
        if !set.insert(id) {
            return None;
        }
        Some(Self {
            set: &ctx.watched_syncing,
            id,
        })
    }
}

impl Drop for SyncGuard<'_> {
    fn drop(&mut self) {
        self.set.lock().unwrap().remove(&self.id);
    }
}

struct FileEntry {
    key: String,
    path: String,
    file_type: &'static str,
    size: i64,
    mtime: i64,
}

/// 非递归列举目录下合规文件，身份键为 lowercase 相对文件名。
fn scan_directory(dir: &str) -> Result<Vec<FileEntry>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("读取监听目录失败: {dir}"))?
    {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let Some(file_type) = wallpaper_service::detect_file_type(ext) else {
            continue;
        };
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let meta = entry.metadata()?;
        let mtime = meta
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH)
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        out.push(FileEntry {
            key: name.to_lowercase(),
            path: path.to_string_lossy().to_string(),
            file_type,
            size: meta.len() as i64,
            mtime,
        });
    }
    Ok(out)
}

/// 幂等扫描。严格同步口径只针对 `watched_folder_id = ?` 的 active 行；
/// 本地导入壁纸与项目回收站完全不参与。
pub async fn sync_folder(ctx: &AppContext, folder_id: i32, force: bool) -> Result<SyncReport> {
    let Some(_guard) = SyncGuard::acquire(ctx, folder_id) else {
        log::debug!("[Watched] folder {folder_id} 正在同步，跳过本次触发");
        return Ok(SyncReport::default());
    };

    let db = &ctx.db;
    let folder = watched_folder::Entity::find_by_id(folder_id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow!("监听目录不存在: {folder_id}"))?;

    if folder.valid == 0 {
        return Err(anyhow!(ERR_PATH_INVALID));
    }
    if !Path::new(&folder.path).is_dir() {
        invalidate_folder(ctx, folder_id).await?;
        return Err(anyhow!(ERR_PATH_INVALID));
    }

    // 后台轮询在当前夹为 active 时跳过完整 diff（存活探测仍常跑）；
    // 进入夹 / 手动刷新走 force=true，不受暂停影响。
    if !force {
        let active = ctx.active_collection_id.load(Ordering::Relaxed);
        if folder.collection_id == Some(active) {
            return Ok(SyncReport::default());
        }
    }

    let entries = scan_directory(&folder.path)?;
    let wanted: HashMap<&str, &FileEntry> = entries.iter().map(|e| (e.key.as_str(), e)).collect();

    let existing = wallpaper::Entity::find()
        .filter(wallpaper::Column::WatchedFolderId.eq(folder_id))
        .all(db)
        .await?;

    let mut report = SyncReport::default();
    let mut seen: HashSet<String> = HashSet::new();

    // 1) 消失的文件：硬删（含缩略图、monitor 引用、关联行）
    for model in &existing {
        let key = model.source_key.clone().unwrap_or_default().to_lowercase();
        if wanted.contains_key(key.as_str()) {
            continue;
        }
        // 批量失效时逐条 reconcile 会反复抢 manager 锁，先只清库，末尾统一一次。
        hard_delete_watched_wallpaper(ctx, model, false).await?;
        report.removed += 1;
    }
    if report.removed > 0 {
        reconcile_after_change(ctx).await;
    }

    // 2) 新增 / 变更
    let cid = folder
        .collection_id
        .ok_or_else(|| anyhow!("监听目录 {folder_id} 未绑定收藏夹"))?;
    for entry in &entries {
        if !seen.insert(entry.key.clone()) {
            continue; // 同名不同扩展名已按 lowercase 文件名去重
        }
        let prior = existing
            .iter()
            .find(|w| w.source_key.as_deref().unwrap_or("").to_lowercase() == entry.key);

        match prior {
            Some(model) => {
                let changed = model.source_size != Some(entry.size)
                    || model.source_mtime != Some(entry.mtime)
                    || model.file_path != entry.path;
                if !changed {
                    continue;
                }
                let mut active: wallpaper::ActiveModel = model.clone().into();
                active.file_path = Set(entry.path.clone());
                active.file_size = Set(Some(entry.size));
                active.source_size = Set(Some(entry.size));
                active.source_mtime = Set(Some(entry.mtime));
                active.update(db).await?;
                if entry.file_type != "video" {
                    refresh_thumbnail(ctx, model.id, entry).await?;
                }
                report.updated += 1;
            }
            None => {
                let model = insert_watched_wallpaper(ctx, folder_id, entry).await?;
                append_to_collection(db, cid, model.id).await?;
                if entry.file_type == "video" {
                    // 后台扫描不抽帧；进夹/刷新时前端按 new_video_ids 补。
                    report.new_video_ids.push(model.id);
                } else {
                    refresh_thumbnail(ctx, model.id, entry).await?;
                }
                report.added += 1;
            }
        }
    }

    let mut active: watched_folder::ActiveModel = folder.into();
    active.last_scan_at = Set(Some(now_str()));
    active.update(db).await?;

    Ok(report)
}

async fn insert_watched_wallpaper(
    ctx: &AppContext,
    folder_id: i32,
    entry: &FileEntry,
) -> Result<wallpaper::Model> {
    let name = Path::new(&entry.path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&entry.key)
        .to_string();

    Ok(wallpaper::ActiveModel {
        name: Set(name),
        r#type: Set(entry.file_type.to_string()),
        file_path: Set(entry.path.clone()),
        file_size: Set(Some(entry.size)),
        is_external: Set(1),
        watched_folder_id: Set(Some(folder_id)),
        source_key: Set(Some(entry.key.clone())),
        source_size: Set(Some(entry.size)),
        source_mtime: Set(Some(entry.mtime)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?)
}

/// 图片/GIF 缩略图按 `wallpaper_id` 命名，避免不同目录同名文件互相覆盖。
async fn refresh_thumbnail(ctx: &AppContext, wallpaper_id: i32, entry: &FileEntry) -> Result<()> {
    let dir = ctx.app_handle.path().app_data_dir()?.join("thumbnails");
    wallpaper_service::ensure_dir(&dir)?;
    let path = wallpaper_service::thumbnail_path_for_id(&dir, wallpaper_id);
    wallpaper_service::generate_static_thumbnail(Path::new(&entry.path), &path)?;

    let active: wallpaper::ActiveModel = wallpaper::ActiveModel {
        id: Set(wallpaper_id),
        thumb_path: Set(Some(path.to_string_lossy().to_string())),
        ..Default::default()
    };
    active.update(&ctx.db).await?;
    Ok(())
}

async fn append_to_collection(
    db: &DatabaseConnection,
    collection_id: i32,
    wallpaper_id: i32,
) -> Result<()> {
    let max = collection_wallpaper::Entity::find()
        .filter(collection_wallpaper::Column::CollectionId.eq(collection_id))
        .order_by_desc(collection_wallpaper::Column::SortOrder)
        .one(db)
        .await?
        .map(|m| m.sort_order)
        .unwrap_or(-1);

    collection_wallpaper::ActiveModel {
        collection_id: Set(collection_id),
        wallpaper_id: Set(wallpaper_id),
        sort_order: Set(max + 1),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(())
}

/// 删除单条监听壁纸（严格同步 / P6 删除链路共用）：源文件由调用方处理，
/// 这里只清 DB 与缩略图，并清 monitor 引用。绝不调用 delete_batch。
pub async fn hard_delete_watched_wallpaper(
    ctx: &AppContext,
    model: &wallpaper::Model,
    do_reconcile: bool,
) -> Result<()> {
    let txn = ctx.db.begin().await?;

    monitor_config::Entity::update_many()
        .col_expr(
            monitor_config::Column::WallpaperId,
            Expr::value(sea_orm::Value::Int(None)),
        )
        .filter(monitor_config::Column::WallpaperId.eq(model.id))
        .exec(&txn)
        .await?;
    collection_wallpaper::Entity::delete_many()
        .filter(collection_wallpaper::Column::WallpaperId.eq(model.id))
        .exec(&txn)
        .await?;
    wallpaper_tag::Entity::delete_many()
        .filter(wallpaper_tag::Column::WallpaperId.eq(model.id))
        .exec(&txn)
        .await?;
    wallpaper::Entity::delete_by_id(model.id).exec(&txn).await?;
    txn.commit().await?;

    if let Some(thumb) = model.thumb_path.as_ref() {
        if let Err(e) = std::fs::remove_file(thumb) {
            log::warn!("[Watched] 清理缩略图失败 {thumb}: {e}");
        }
    }

    if do_reconcile {
        reconcile_after_change(ctx).await;
    }
    Ok(())
}

/// 监听链路改动 monitor 引用后的统一收尾：一次 reconcile 覆盖所有屏。
async fn reconcile_after_change(ctx: &AppContext) {
    let mut guard = ctx.unified_manager.lock().await;
    if let Err(e) = guard.reconcile(ReconcileScope::All).await {
        log::warn!("[Watched] 删除联动 reconcile 失败: {e}");
    }
    guard.notify_config_refreshed();
}

// ==================== 源文件移出（P6 链路 / P7 Windows 实现） ====================

/// 把监听目录里的源文件移入回收站的统一 port（异步，Windows 侧内部 spawn_blocking）。
///
/// - Windows：`SHFileOperationW` + `FOF_ALLOWUNDO`，双 NUL 结尾路径，进系统回收站。
/// - 非 Windows：移到 app data 的 `watched-trash/`（可逆、模拟回收站），
///   使 macOS 能完整验证「删除 → 硬删 DB → 同步严格性」而不真删文件。
///
/// 应用**不负责**从系统回收站还原；用户手动还原后由扫描当新增重新收录。
#[cfg(not(target_os = "windows"))]
pub async fn recycle_source_file(ctx: &AppContext, path: &Path) -> Result<()> {
    let trash_dir = ctx.app_handle.path().app_data_dir()?.join("watched-trash");
    std::fs::create_dir_all(&trash_dir)?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");
    let dest = trash_dir.join(format!("{}_{}", uuid::Uuid::new_v4(), name));
    std::fs::rename(path, &dest).with_context(|| format!("移出源文件失败: {path:?}"))?;
    Ok(())
}

#[cfg(target_os = "windows")]
pub async fn recycle_source_file(_ctx: &AppContext, path: &Path) -> Result<()> {
    let path = path.to_path_buf();
    // SHFileOperationW 会等待 Shell 响应，不能占着 tokio worker。
    tauri::async_runtime::spawn_blocking(move || {
        crate::platform::windows::recycle_bin::recycle_file(&path)
    })
    .await
    .context("回收站阻塞任务被取消")??;
    Ok(())
}

/// 监听壁纸删除：先移出源文件，成功才硬删 DB；失败跳过该条，
/// 避免「文件还在但记录没了」导致下次扫描把它当新增复活。
/// 批量逐条处理，不用整批事务。
pub async fn delete_watched_wallpapers(ctx: &AppContext, ids: Vec<i32>) -> Result<usize> {
    let mut deleted = 0usize;
    for id in ids {
        let Some(model) = wallpaper::Entity::find_by_id(id).one(&ctx.db).await? else {
            continue;
        };
        if model.is_external != 1 {
            bail!("只能删除监听壁纸: {id}");
        }
        match recycle_source_file(ctx, Path::new(&model.file_path)).await {
            Ok(()) => {}
            Err(e) => {
                log::warn!("[Watched] 跳过删除 #{}: {e:#}", id);
                continue;
            }
        }
        hard_delete_watched_wallpaper(ctx, &model, true).await?;
        deleted += 1;
    }
    Ok(deleted)
}

// ==================== P3 触发源 ====================

pub async fn scan_interval_seconds(db: &DatabaseConnection) -> u64 {
    let minutes = app_setting_service::get(db, SETTING_SCAN_INTERVAL_MIN)
        .await
        .ok()
        .flatten()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_SCAN_INTERVAL_MIN)
        .clamp(1, 60);
    minutes * 60
}

/// 存活探测对所有有效监听常跑（不受 active 暂停影响），
/// 否则「有效→失效」这个转变没人能发现。
pub async fn probe_and_sync(ctx: &AppContext) -> Result<()> {
    let folders = watched_folder::Entity::find()
        .filter(watched_folder::Column::Valid.eq(1))
        .all(&ctx.db)
        .await?;

    for folder in folders {
        if !Path::new(&folder.path).is_dir() {
            log::warn!("[Watched] 目录失效，清库删夹: {}", folder.path);
            if let Err(e) = invalidate_folder(ctx, folder.id).await {
                log::warn!("[Watched] 失效清理失败: {e:#}");
            }
            continue;
        }
        if let Err(e) = sync_folder(ctx, folder.id, false).await {
            log::warn!("[Watched] 后台同步失败 folder={}: {e:#}", folder.id);
        }
    }
    Ok(())
}

/// 前端进入夹 / 手动刷新：按 collection_id 找到监听目录，强制同步。
/// 路径失效会把 ERR_PATH_INVALID 上抛，由前端 toast + activeId=0。
pub async fn sync_by_collection(ctx: &AppContext, collection_id: i32) -> Result<SyncReport> {
    let Some(folder) = find_by_collection(&ctx.db, collection_id).await? else {
        return Ok(SyncReport::default());
    };
    if folder.valid == 0 {
        return Err(anyhow!(ERR_PATH_INVALID));
    }
    sync_folder(ctx, folder.id, true).await
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "mini-wallpaper-watched-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 非递归 + 扩展名白名单 + 身份键 lowercase 相对文件名。
    #[test]
    fn scan_directory_is_non_recursive_and_filtered() {
        let dir = TempDir::new();
        std::fs::write(dir.0.join("A.PNG"), b"x").unwrap();
        std::fs::write(dir.0.join("clip.mp4"), b"x").unwrap();
        std::fs::write(dir.0.join("notes.txt"), b"x").unwrap();
        std::fs::create_dir(dir.0.join("sub")).unwrap();
        std::fs::write(dir.0.join("sub").join("inner.jpg"), b"x").unwrap();

        let entries = scan_directory(dir.0.to_str().unwrap()).unwrap();
        assert_eq!(entries.len(), 2, "子目录与不支持格式都必须跳过");

        let by_key: HashMap<&str, &FileEntry> =
            entries.iter().map(|e| (e.key.as_str(), e)).collect();
        assert_eq!(by_key["a.png"].file_type, "image");
        assert_eq!(by_key["clip.mp4"].file_type, "video");
        assert!(by_key["a.png"].size > 0);
        assert!(by_key["a.png"].mtime > 0);
    }
}
