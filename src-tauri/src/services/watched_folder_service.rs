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
    sea_query::Expr, ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, QueryFilter, QueryOrder, Set, TransactionTrait,
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
/// 纯同步 syscall + 元数据读取，调用方必须在 `spawn_blocking` 中执行。
fn scan_directory(dir: &Path) -> Result<Vec<FileEntry>> {
    let mut out = Vec::new();
    for entry in
        std::fs::read_dir(dir).with_context(|| format!("读取监听目录失败: {}", dir.display()))?
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

/// 一次扫描的差集结果。`plan_sync` 是纯函数，便于单测覆盖「新增 / 变更 / 删除」
/// 三种判定，而不必构造 AppContext。
#[derive(Default)]
struct SyncPlan {
    new_files: Vec<FileEntry>,
    changed: Vec<(wallpaper::Model, FileEntry)>,
    removed: Vec<wallpaper::Model>,
}

/// 存量按 source_key 建 HashMap；扫描侧每命中一条就 remove，
/// 遍历完仍留在 map 里的就是磁盘上已消失的行。整体 O(N+M)，
/// 替代原先「对每条扫描结果线性 find 存量」的 O(N×M)。
///
/// source_key 是 lowercase 文件名，size/mtime 变化仍命中同一条并归到 changed；
/// 只有文件名变化才会表现成「旧 key 判删 + 新 key 判增」。
fn plan_sync(entries: Vec<FileEntry>, existing: Vec<wallpaper::Model>) -> SyncPlan {
    let mut by_key: HashMap<String, wallpaper::Model> = HashMap::new();
    let mut plan = SyncPlan::default();

    for model in existing {
        match model.source_key.as_deref() {
            Some(key) if !key.is_empty() => {
                by_key.insert(key.to_lowercase(), model);
            }
            // 无身份键的脏行不对应任何扫描结果，直接按删除处理
            _ => plan.removed.push(model),
        }
    }

    let mut seen: HashSet<String> = HashSet::new();
    for entry in entries {
        // 同名不同扩展名按 lowercase 文件名去重，保留扫描顺序中的第一个
        if !seen.insert(entry.key.clone()) {
            continue;
        }
        match by_key.remove(&entry.key) {
            None => plan.new_files.push(entry),
            Some(model) => {
                let changed = model.source_size != Some(entry.size)
                    || model.source_mtime != Some(entry.mtime)
                    || model.file_path != entry.path;
                if changed {
                    plan.changed.push((model, entry));
                }
            }
        }
    }

    plan.removed.extend(by_key.into_values());
    plan
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

    // 目录扫描是纯同步 syscall（每文件 is_file + metadata），占住 tokio worker
    // 会拖慢其它 command；挪到 blocking 线程池。
    let scan_path = PathBuf::from(&folder.path);
    let entries = tauri::async_runtime::spawn_blocking(move || scan_directory(&scan_path))
        .await
        .context("目录扫描任务被取消")??;

    let rows = wallpaper::Entity::find()
        .filter(wallpaper::Column::WatchedFolderId.eq(folder_id))
        .all(db)
        .await?;
    let plan = plan_sync(entries, rows);

    let cid = folder
        .collection_id
        .ok_or_else(|| anyhow!("监听目录 {folder_id} 未绑定收藏夹"))?;
    let mut report = SyncReport {
        added: plan.new_files.len(),
        updated: plan.changed.len(),
        removed: plan.removed.len(),
        new_video_ids: Vec::new(),
    };
    // 只记录「新文件」生成的缩略图：回滚后这些 id 不存在，删掉才是安全的。
    // 变更文件复用 {id}.webp，回滚后旧记录仍指向它，绝不能清理。
    let mut rollback_thumbs: Vec<PathBuf> = Vec::new();
    let mut removed_thumbs: Vec<String> = Vec::new();

    // 整轮写操作合并为单个事务：任一处失败全部回滚，不留半截同步状态。
    let txn = db.begin().await?;

    // 1) 新文件
    for entry in &plan.new_files {
        let model = insert_watched_wallpaper(&txn, folder_id, entry).await?;
        append_to_collection(&txn, cid, model.id).await?;
        if entry.file_type == "video" {
            // 后台扫描不抽帧；进夹/刷新时前端按 new_video_ids 补。
            report.new_video_ids.push(model.id);
        } else {
            let thumb = generate_thumbnail_file(ctx, model.id, entry).await?;
            set_thumbnail_path(&txn, model.id, &thumb).await?;
            rollback_thumbs.push(thumb);
        }
    }

    // 2) 内容变更（source_key 不变，size / mtime / path 变化）
    for (model, entry) in &plan.changed {
        let mut active: wallpaper::ActiveModel = model.clone().into();
        active.file_path = Set(entry.path.clone());
        active.file_size = Set(Some(entry.size));
        active.source_size = Set(Some(entry.size));
        active.source_mtime = Set(Some(entry.mtime));
        active.update(&txn).await?;
        if entry.file_type != "video" {
            let thumb = generate_thumbnail_file(ctx, model.id, entry).await?;
            set_thumbnail_path(&txn, model.id, &thumb).await?;
            // 复用旧 id 的路径，回滚后旧记录仍指向它，不加入清理列表
        }
    }

    // 3) 磁盘上已不存在；硬删（monitor 引用 / 关联行 / 标签 / 行）
    for model in &plan.removed {
        if let Some(thumb) = model.thumb_path.as_ref() {
            removed_thumbs.push(thumb.clone());
        }
        delete_watched_row(&txn, model).await?;
    }

    let mut active: watched_folder::ActiveModel = folder.into();
    active.last_scan_at = Set(Some(now_str()));
    active.update(&txn).await?;

    if let Err(e) = txn.commit().await {
        // 只清理新文件那批（回滚后无记录引用）；变更文件复用旧路径，不能删。
        for thumb in rollback_thumbs {
            let _ = std::fs::remove_file(thumb);
        }
        return Err(e.into());
    }

    // 事务外副作用：文件删除 + 一次 reconcile（避免逐条抢 manager 锁）
    for thumb in removed_thumbs {
        if let Err(e) = std::fs::remove_file(&thumb) {
            log::warn!("[Watched] 清理缩略图失败 {thumb}: {e}");
        }
    }
    if report.removed > 0 {
        reconcile_after_change(ctx).await;
    }

    Ok(report)
}

async fn insert_watched_wallpaper<C: ConnectionTrait>(
    txn: &C,
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
    .insert(txn)
    .await?)
}

/// 图片/GIF 缩略图按 `wallpaper_id` 命名，避免不同目录同名文件互相覆盖。
///
/// `image::open → thumbnail → WebP save` 是同步 CPU 密集操作，必须放进
/// blocking 线程池，否则一次拷入大量图片会独占 tokio worker 数秒。
async fn generate_thumbnail_file(
    ctx: &AppContext,
    wallpaper_id: i32,
    entry: &FileEntry,
) -> Result<PathBuf> {
    let dir = ctx.app_handle.path().app_data_dir()?.join("thumbnails");
    let thumb_path = wallpaper_service::thumbnail_path_for_id(&dir, wallpaper_id);
    let source = PathBuf::from(&entry.path);
    let target = thumb_path.clone();

    tauri::async_runtime::spawn_blocking(move || {
        wallpaper_service::ensure_dir(
            target
                .parent()
                .ok_or_else(|| anyhow!("缩略图路径没有父目录: {target:?}"))?,
        )?;
        wallpaper_service::generate_static_thumbnail(&source, &target)
    })
    .await
    .context("缩略图生成任务被取消")??;

    Ok(thumb_path)
}

async fn set_thumbnail_path<C: ConnectionTrait>(
    txn: &C,
    wallpaper_id: i32,
    path: &Path,
) -> Result<()> {
    let active: wallpaper::ActiveModel = wallpaper::ActiveModel {
        id: Set(wallpaper_id),
        thumb_path: Set(Some(path.to_string_lossy().to_string())),
        ..Default::default()
    };
    active.update(txn).await?;
    Ok(())
}

async fn append_to_collection<C: ConnectionTrait>(
    txn: &C,
    collection_id: i32,
    wallpaper_id: i32,
) -> Result<()> {
    let max = collection_wallpaper::Entity::find()
        .filter(collection_wallpaper::Column::CollectionId.eq(collection_id))
        .order_by_desc(collection_wallpaper::Column::SortOrder)
        .one(txn)
        .await?
        .map(|m| m.sort_order)
        .unwrap_or(-1);

    collection_wallpaper::ActiveModel {
        collection_id: Set(collection_id),
        wallpaper_id: Set(wallpaper_id),
        sort_order: Set(max + 1),
        ..Default::default()
    }
    .insert(txn)
    .await?;
    Ok(())
}

/// 删除单行的 DB 副作用；由调用方决定事务边界（sync_folder 用整轮大事务，
/// P6 单条删除用独立小事务）。源文件由调用方处理，绝不调用 delete_batch。
async fn delete_watched_row<C: ConnectionTrait>(txn: &C, model: &wallpaper::Model) -> Result<()> {
    monitor_config::Entity::update_many()
        .col_expr(
            monitor_config::Column::WallpaperId,
            Expr::value(sea_orm::Value::Int(None)),
        )
        .filter(monitor_config::Column::WallpaperId.eq(model.id))
        .exec(txn)
        .await?;
    collection_wallpaper::Entity::delete_many()
        .filter(collection_wallpaper::Column::WallpaperId.eq(model.id))
        .exec(txn)
        .await?;
    wallpaper_tag::Entity::delete_many()
        .filter(wallpaper_tag::Column::WallpaperId.eq(model.id))
        .exec(txn)
        .await?;
    wallpaper::Entity::delete_by_id(model.id).exec(txn).await?;
    Ok(())
}

/// P6 单条删除入口：独立事务 + 缩略图清理 + 一次联动。
pub async fn hard_delete_watched_wallpaper(
    ctx: &AppContext,
    model: &wallpaper::Model,
    do_reconcile: bool,
) -> Result<()> {
    let txn = ctx.db.begin().await?;
    delete_watched_row(&txn, model).await?;
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

    fn fake_model(key: Option<&str>, size: i64, mtime: i64) -> wallpaper::Model {
        let suffix = key.unwrap_or("nokey");
        wallpaper::Model {
            id: 0,
            name: suffix.to_string(),
            r#type: "image".to_string(),
            file_path: format!("/w/{suffix}"),
            thumb_path: None,
            width: None,
            height: None,
            duration: None,
            file_size: Some(size),
            play_count: 0,
            created_at: String::new(),
            updated_at: String::new(),
            deleted_at: None,
            is_external: 1,
            watched_folder_id: Some(1),
            source_key: key.map(str::to_string),
            source_size: Some(size),
            source_mtime: Some(mtime),
        }
    }

    fn fake_entry(name: &str, size: i64, mtime: i64) -> FileEntry {
        let key = name.to_lowercase();
        FileEntry {
            key: key.clone(),
            path: format!("/w/{key}"),
            file_type: "image",
            size,
            mtime,
        }
    }

    /// 差集判定：未变跳过、size/mtime 变化归更新、key 消失归删除、
    /// 无 key 脏行归删除、大小写重复的 key 只处理一次。
    #[test]
    fn plan_sync_classifies_added_changed_removed_and_dedups() {
        let existing = vec![
            fake_model(Some("keep.jpg"), 100, 10),
            fake_model(Some("changed.jpg"), 100, 10),
            fake_model(Some("gone.jpg"), 100, 10),
            fake_model(Some("dupe.png"), 100, 10),
            fake_model(None, 100, 10),
        ];
        let entries = vec![
            fake_entry("keep.jpg", 100, 10),    // 未变
            fake_entry("changed.jpg", 200, 11), // 内容变更
            fake_entry("new.jpg", 300, 12),     // 新文件
            fake_entry("DUPE.PNG", 100, 10),    // 命中存量，大小写差异
            fake_entry("dupe.png", 100, 10),    // 同 key，被 seen 跳过
        ];

        let plan = plan_sync(entries, existing);

        assert_eq!(plan.new_files.len(), 1);
        assert_eq!(plan.new_files[0].key, "new.jpg");
        assert_eq!(plan.changed.len(), 1);
        assert_eq!(plan.changed[0].0.source_key.as_deref(), Some("changed.jpg"));
        assert_eq!(plan.removed.len(), 2);
        let mut removed_keys: Vec<_> = plan
            .removed
            .iter()
            .map(|m| m.source_key.clone().unwrap_or_default())
            .collect();
        removed_keys.sort();
        assert_eq!(removed_keys, vec!["", "gone.jpg"]);
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

        let entries = scan_directory(&dir.0).unwrap();
        assert_eq!(entries.len(), 2, "子目录与不支持格式都必须跳过");

        let by_key: HashMap<&str, &FileEntry> =
            entries.iter().map(|e| (e.key.as_str(), e)).collect();
        assert_eq!(by_key["a.png"].file_type, "image");
        assert_eq!(by_key["clip.mp4"].file_type, "video");
        assert!(by_key["a.png"].size > 0);
        assert!(by_key["a.png"].mtime > 0);
    }
}
