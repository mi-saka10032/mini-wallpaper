use anyhow::{Context, Result};
use futures::stream::{self, StreamExt};
use image::GenericImageView;
use log::{info, warn};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, Select, Set,
};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::entities::{collection_wallpaper, monitor_config, wallpaper};
use crate::utils::concurrency::import_concurrency;

/// 支持的图片扩展名
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "bmp", "webp"];

/// 支持的视频扩展名
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "webm", "mkv", "avi", "mov"];

/// 支持的 GIF 扩展名
const GIF_EXTENSIONS: &[&str] = &["gif"];

// ==================== 软删除统一过滤入口 ====================
//
// 回收站引入后，壁纸存在「正常」与「已移入回收站」两种状态。全仓所有壁纸
// 查询都必须显式选择其中一种视图，任何一处遗漏都会让回收站内的壁纸从主网格、
// 智能收藏夹或轮播中「诈尸」。因此过滤逻辑收敛到本节的三个函数，
// 禁止在各 service 里手写 `deleted_at` 字面条件。

/// 未删除条件：`deleted_at IS NULL`
///
/// 供需要自行拼装 `Condition` 的场景复用（如智能收藏夹规则引擎，
/// 它已有一个由 rule_json 编译出的 Condition，只需再 and 上本条件）。
pub fn not_deleted() -> Condition {
    Condition::all().add(wallpaper::Column::DeletedAt.is_null())
}

/// 已删除条件：`deleted_at IS NOT NULL`
pub fn is_deleted() -> Condition {
    Condition::all().add(wallpaper::Column::DeletedAt.is_not_null())
}

/// 默认视图：仅正常壁纸（不含回收站）
///
/// 主网格、收藏夹成员、轮播、标签计数等一切面向用户的常规链路都应使用它。
pub fn active() -> Select<wallpaper::Entity> {
    wallpaper::Entity::find().filter(not_deleted())
}

/// 回收站视图：仅已移入回收站的壁纸
pub fn trashed() -> Select<wallpaper::Entity> {
    wallpaper::Entity::find().filter(is_deleted())
}

/// 获取所有壁纸（不含回收站）
pub async fn get_all(db: &DatabaseConnection) -> Result<Vec<wallpaper::Model>> {
    active()
        .all(db)
        .await
        .context("Failed to fetch wallpapers")
}

/// 获取回收站内的壁纸（按移入时间倒序，最近删除的在前）
pub async fn get_trashed(db: &DatabaseConnection) -> Result<Vec<wallpaper::Model>> {
    trashed()
        .order_by_desc(wallpaper::Column::DeletedAt)
        .all(db)
        .await
        .context("Failed to fetch trashed wallpapers")
}

/// 根据 ID 获取单个壁纸详情（**含回收站内的记录**）
///
/// 恢复、彻底删除等回收站操作需要读取已删记录，故此处不加过滤。
/// 播放链路请改用 `get_active_by_id`，避免把回收站内的壁纸设为桌面壁纸。
pub async fn get_by_id(db: &DatabaseConnection, id: i32) -> Result<Option<wallpaper::Model>> {
    wallpaper::Entity::find_by_id(id)
        .one(db)
        .await
        .context("Failed to fetch wallpaper by id")
}

/// 根据 ID 获取单个正常壁纸（回收站内的视为不存在）
pub async fn get_active_by_id(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<wallpaper::Model>> {
    active()
        .filter(wallpaper::Column::Id.eq(id))
        .one(db)
        .await
        .context("Failed to fetch active wallpaper by id")
}

/// 获取所有支持的壁纸文件扩展名
pub fn get_supported_extensions() -> Vec<String> {
    IMAGE_EXTENSIONS
        .iter()
        .chain(VIDEO_EXTENSIONS.iter())
        .chain(GIF_EXTENSIONS.iter())
        .map(|s| s.to_string())
        .collect()
}

/// 判断文件类型
fn detect_file_type(ext: &str) -> Option<&'static str> {
    let ext = ext.to_lowercase();
    if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        Some("image")
    } else if VIDEO_EXTENSIONS.contains(&ext.as_str()) {
        Some("video")
    } else if GIF_EXTENSIONS.contains(&ext.as_str()) {
        Some("gif")
    } else {
        None
    }
}

/// 确保目录存在
fn ensure_dir(dir: &Path) -> Result<()> {
    if !dir.exists() {
        std::fs::create_dir_all(dir).context("Failed to create directory")?;
    }
    Ok(())
}

/// 复制文件到应用目录，返回新路径
fn copy_to_app_dir(source: &Path, wallpapers_dir: &Path) -> Result<PathBuf> {
    ensure_dir(wallpapers_dir)?;

    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");
    let new_name = format!("{}.{}", Uuid::new_v4(), ext);
    let dest = wallpapers_dir.join(&new_name);

    std::fs::copy(source, &dest).context("Failed to copy wallpaper file")?;

    Ok(dest)
}

/// 生成图片/GIF 缩略图（image crate，等比缩放最大宽度 480px）
fn generate_static_thumbnail(source: &Path, thumb_path: &Path) -> Result<()> {
    let img = image::open(source).context("Failed to open image")?;
    let thumbnail = img.thumbnail(480, 480);
    thumbnail
        .save(thumb_path)
        .context("Failed to save thumbnail")?;
    Ok(())
}

/// 获取图片尺寸
fn get_image_dimensions(path: &Path) -> Option<(u32, u32)> {
    image::open(path).ok().map(|img| img.dimensions())
}

/// 文件预处理结果（纯同步 I/O 阶段产出）
struct PreparedWallpaper {
    original_name: String,
    file_type: String,
    dest_path: String,
    thumb_path: Option<String>,
    width: Option<i32>,
    height: Option<i32>,
    file_size: i64,
}

/// 同步文件预处理：校验、复制、生成缩略图、获取尺寸
///
/// 该函数包含所有阻塞 I/O 操作（文件复制、图片解码/编码），
/// 应在 `spawn_blocking` 中调用以避免阻塞 async runtime。
fn prepare_wallpaper_files(
    source_path: &str,
    wallpapers_dir: &Path,
    thumbnails_dir: &Path,
) -> Result<PreparedWallpaper> {
    let source = Path::new(source_path);

    // 1. 检查文件存在
    if !source.exists() {
        anyhow::bail!("File not found: {}", source_path);
    }

    // 2. 检查文件类型
    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let file_type = detect_file_type(ext)
        .ok_or_else(|| anyhow::anyhow!("Unsupported file type: .{}", ext))?;

    // 3. 获取原始文件名
    let original_name = source
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    // 4. 获取文件大小
    let file_size = std::fs::metadata(source)
        .map(|m| m.len() as i64)
        .unwrap_or(0);

    // 5. 复制文件到应用目录
    let dest_path = copy_to_app_dir(source, wallpapers_dir)?;

    // 6. 生成缩略图：仅图片/GIF 在此生成，视频由前端 canvas 抽帧后单独写入
    let thumb_path_str = if file_type == "image" || file_type == "gif" {
        ensure_dir(thumbnails_dir)?;
        let thumb_name = format!(
            "{}.webp",
            dest_path.file_stem().expect("dest_path must have a file stem").to_string_lossy(),
        );
        let thumb_path = thumbnails_dir.join(&thumb_name);
        match generate_static_thumbnail(&dest_path, &thumb_path) {
            Ok(()) => {
                info!("[Thumbnail] Generated: {:?}", thumb_path);
                Some(thumb_path.to_string_lossy().to_string())
            }
            Err(e) => {
                warn!("[WARN] Thumbnail generation failed for {}: {}", original_name, e);
                None
            }
        }
    } else {
        None
    };

    // 7. 获取图片/GIF 尺寸（视频尺寸暂不提取）
    let (width, height) = if file_type == "image" || file_type == "gif" {
        get_image_dimensions(&dest_path)
            .map(|(w, h)| (Some(w as i32), Some(h as i32)))
            .unwrap_or((None, None))
    } else {
        (None, None)
    };

    Ok(PreparedWallpaper {
        original_name,
        file_type: file_type.to_string(),
        dest_path: dest_path.to_string_lossy().to_string(),
        thumb_path: thumb_path_str,
        width,
        height,
        file_size,
    })
}

/// 导入单个壁纸文件
///
/// 分为两个阶段：
/// 1. **文件预处理**（spawn_blocking）：文件复制、缩略图生成等阻塞 I/O
/// 2. **数据库写入**（async）：将预处理结果插入数据库
///
/// 视频缩略图由前端 canvas 抽帧后通过 `save_video_thumbnail` 单独写入。
pub async fn import_single(
    db: &DatabaseConnection,
    source_path: &str,
    wallpapers_dir: &Path,
    thumbnails_dir: &Path,
) -> Result<wallpaper::Model> {
    // 阶段 1：在 blocking 线程池中执行所有同步 I/O
    let source_path_owned = source_path.to_string();
    let wallpapers_dir_owned = wallpapers_dir.to_path_buf();
    let thumbnails_dir_owned = thumbnails_dir.to_path_buf();

    let prepared = tokio::task::spawn_blocking(move || {
        prepare_wallpaper_files(&source_path_owned, &wallpapers_dir_owned, &thumbnails_dir_owned)
    })
    .await
    .context("File preparation task panicked")??;

    // 阶段 2：异步写入数据库
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let active_model = wallpaper::ActiveModel {
        name: Set(prepared.original_name),
        r#type: Set(prepared.file_type),
        file_path: Set(prepared.dest_path.clone()),
        thumb_path: Set(prepared.thumb_path),
        width: Set(prepared.width),
        height: Set(prepared.height),
        duration: Set(None),
        file_size: Set(Some(prepared.file_size)),
        play_count: Set(0),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    };

    let model = active_model
        .insert(db)
        .await
        .context("Failed to insert wallpaper into database")?;

    info!("[Import] {} -> {}", source_path, model.file_path);

    Ok(model)
}

/// 批量导入壁纸（有限并发）
///
/// 使用 `buffer_unordered` 控制最大并发数（基于 CPU 核数动态计算），
/// 每个任务内部通过 `spawn_blocking` 执行文件 I/O，避免阻塞 async runtime。
/// SQLite 写操作由连接池自动排队，无需额外加锁。

pub async fn import_batch(
    db: &DatabaseConnection,
    source_paths: Vec<String>,
    wallpapers_dir: &Path,
    thumbnails_dir: &Path,
) -> Result<Vec<wallpaper::Model>> {
    let wallpapers_dir = wallpapers_dir.to_path_buf();
    let thumbnails_dir = thumbnails_dir.to_path_buf();

    let results: Vec<std::result::Result<wallpaper::Model, (String, anyhow::Error)>> =
        stream::iter(source_paths)
            .map(|path| {
                let w_dir = wallpapers_dir.clone();
                let t_dir = thumbnails_dir.clone();
                async move {
                    import_single(db, &path, &w_dir, &t_dir)
                        .await
                        .map_err(|e| (path, e))
                }
            })
            .buffer_unordered(import_concurrency())
            .collect()
            .await;

    let mut models = Vec::new();
    let mut errors = Vec::new();

    for result in results {
        match result {
            Ok(model) => models.push(model),
            Err((path, e)) => {
                warn!("[Import Error] {}: {}", path, e);
                errors.push(format!("{}: {}", path, e));
            }
        }
    }

    if models.is_empty() && !errors.is_empty() {
        anyhow::bail!("All imports failed: {}", errors.join("; "));
    }

    Ok(models)
}

/// 字节预处理结果（spawn_blocking 阶段产出）
struct PreparedBytesWallpaper {
    original_name: String,
    file_type: String,
    dest_path: String,
    thumb_path: Option<String>,
    width: Option<i32>,
    height: Option<i32>,
    file_size: i64,
}

/// 同步字节预处理：校验、写盘、生成缩略图、获取尺寸
///
/// 与 `prepare_wallpaper_files` 不同：
/// - 不依赖磁盘原文件路径，直接接收字节内容
/// - 通过 `original_name` 解析扩展名与文件类型
fn prepare_wallpaper_from_bytes(
    original_name: &str,
    bytes: &[u8],
    wallpapers_dir: &Path,
    thumbnails_dir: &Path,
) -> Result<PreparedBytesWallpaper> {
    // 1. 解析扩展名 + 校验文件类型
    let ext = Path::new(original_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let file_type = detect_file_type(ext)
        .ok_or_else(|| anyhow::anyhow!("Unsupported file type: .{}", ext))?;

    // 2. 文件大小（直接用字节长度）
    let file_size = bytes.len() as i64;

    // 3. 写入应用目录（uuid 命名，保留原扩展名）
    ensure_dir(wallpapers_dir)?;
    let new_name = format!("{}.{}", Uuid::new_v4(), ext);
    let dest_path = wallpapers_dir.join(&new_name);
    std::fs::write(&dest_path, bytes).context("Failed to write wallpaper bytes")?;

    // 4. 缩略图：仅图片/GIF 在此生成，视频由前端 canvas 抽帧后单独写入
    let thumb_path_str = if file_type == "image" || file_type == "gif" {
        ensure_dir(thumbnails_dir)?;
        let thumb_name = format!(
            "{}.webp",
            dest_path.file_stem().expect("dest_path must have a file stem").to_string_lossy(),
        );
        let thumb_path = thumbnails_dir.join(&thumb_name);
        match generate_static_thumbnail(&dest_path, &thumb_path) {
            Ok(()) => {
                info!("[Thumbnail] Generated: {:?}", thumb_path);
                Some(thumb_path.to_string_lossy().to_string())
            }
            Err(e) => {
                warn!("[WARN] Thumbnail generation failed for {}: {}", original_name, e);
                None
            }
        }
    } else {
        None
    };

    // 5. 尺寸（图片/GIF 才提取，视频由前端处理）
    let (width, height) = if file_type == "image" || file_type == "gif" {
        get_image_dimensions(&dest_path)
            .map(|(w, h)| (Some(w as i32), Some(h as i32)))
            .unwrap_or((None, None))
    } else {
        (None, None)
    };

    Ok(PreparedBytesWallpaper {
        original_name: original_name.to_string(),
        file_type: file_type.to_string(),
        dest_path: dest_path.to_string_lossy().to_string(),
        thumb_path: thumb_path_str,
        width,
        height,
        file_size,
    })
}

/// 通过字节方式导入单个壁纸（H5 拖拽场景）
///
/// 与 `import_single` 区别：
/// - 不读取磁盘源文件，避免依赖 Tauri 注入的 path 属性
/// - 适用于浏览器 File 对象通过 raw body（Uint8Array）传输到后端的场景
pub async fn import_single_from_bytes(
    db: &DatabaseConnection,
    original_name: String,
    bytes: Vec<u8>,
    wallpapers_dir: &Path,
    thumbnails_dir: &Path,
) -> Result<wallpaper::Model> {
    // 阶段 1：spawn_blocking 处理同步 I/O
    let wallpapers_dir_owned = wallpapers_dir.to_path_buf();
    let thumbnails_dir_owned = thumbnails_dir.to_path_buf();
    let name_for_blocking = original_name.clone();

    let prepared = tokio::task::spawn_blocking(move || {
        prepare_wallpaper_from_bytes(
            &name_for_blocking,
            &bytes,
            &wallpapers_dir_owned,
            &thumbnails_dir_owned,
        )
    })
    .await
    .context("File preparation task panicked")??;

    // 阶段 2：异步写入数据库
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let active_model = wallpaper::ActiveModel {
        name: Set(prepared.original_name),
        r#type: Set(prepared.file_type),
        file_path: Set(prepared.dest_path.clone()),
        thumb_path: Set(prepared.thumb_path),
        width: Set(prepared.width),
        height: Set(prepared.height),
        duration: Set(None),
        file_size: Set(Some(prepared.file_size)),
        play_count: Set(0),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    };

    let model = active_model
        .insert(db)
        .await
        .context("Failed to insert wallpaper into database")?;

    info!("[ImportBytes] {} -> {}", original_name, model.file_path);

    Ok(model)
}

/// 保存前端 canvas 生成的视频缩略图
///
/// 接收前端传来的图片字节数据（WebP/JPEG），持久化到 thumbnails 目录，
/// 并更新对应壁纸记录的 thumb_path。
pub async fn save_video_thumbnail(
    db: &DatabaseConnection,
    wallpaper_id: i32,
    data: Vec<u8>,
    thumbnails_dir: &Path,
) -> Result<String> {
    // 1. 查找壁纸记录
    let model = wallpaper::Entity::find_by_id(wallpaper_id)
        .one(db)
        .await
        .context("Failed to query wallpaper")?
        .ok_or_else(|| anyhow::anyhow!("Wallpaper not found: {}", wallpaper_id))?;

    // 2. 根据壁纸文件名生成缩略图文件名（与壁纸文件同 stem）
    ensure_dir(thumbnails_dir)?;
    let stem = Path::new(&model.file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown_video");
    let thumb_name = format!("{}.webp", stem);
    let thumb_path = thumbnails_dir.join(&thumb_name);

    // 3. 写入文件
    std::fs::write(&thumb_path, &data)
        .context("Failed to write video thumbnail")?;

    let thumb_path_str = thumb_path.to_string_lossy().to_string();

    // 4. 更新数据库 thumb_path
    let mut active: wallpaper::ActiveModel = model.into();
    active.thumb_path = Set(Some(thumb_path_str.clone()));
    active.update(db).await.context("Failed to update wallpaper thumb_path")?;

    info!("[VideoThumbnail] Saved: {}", thumb_path_str);
    Ok(thumb_path_str)
}

/// `sort_order` 哨兵值：标记「该关联行的壁纸已进回收站」
///
/// 软删除时把关联行的 sort_order 置为该值，使可见成员的 sort_order 始终保持
/// 无重复的连续区间 `0..n-1`。这是必需的：`sort_order` 不只是展示顺序，还是
/// 顺序轮播的游标（`find_adjacent_wallpaper` 用严格不等比较取上/下一张），
/// 一旦出现重复值就会静默跳图、或造成 next/prev 不对称。
/// `-1` 天然排在所有可见值之前且不与之冲突。
pub const TRASH_SORT_ORDER: i32 = -1;

/// 移入回收站（软删除，批量；事务保护）
///
/// 只打 DB 标记，不删除磁盘文件，也不解除收藏夹 / 标签关联——以便恢复后壁纸
/// 仍属于原收藏夹、仍带原标签。但仍会清空 `monitor_configs.wallpaper_id`
/// 中对该壁纸的引用，因为正在显示的壁纸进了回收站，桌面必须立刻切走或清空。
///
/// 事务内依次完成：
/// 1. 置 `deleted_at`
/// 2. 关联行 `sort_order` 置哨兵，并对受影响收藏夹的剩余可见成员连续化重排
/// 3. 解除 monitor_configs 引用
///
/// 第 2 步必须与第 1 步同事务，否则中断会留下重复的 sort_order。
pub async fn trash_batch(db: &DatabaseConnection, ids: Vec<i32>) -> Result<u64> {
    use sea_orm::TransactionTrait;

    if ids.is_empty() {
        return Ok(0);
    }

    let txn = db.begin().await?;
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    // 收集受影响的收藏夹，稍后统一连续化重排
    let mut affected_collections: std::collections::HashSet<i32> = std::collections::HashSet::new();
    let mut trashed_count = 0u64;

    for id in &ids {
        // 仅处理存在且尚未在回收站中的壁纸
        let model = wallpaper::Entity::find_by_id(*id)
            .one(&txn)
            .await
            .context("Failed to query wallpaper")?;

        let Some(model) = model else {
            warn!("[Trash] Wallpaper not found: {}", id);
            continue;
        };
        if model.deleted_at.is_some() {
            continue;
        }

        // 1. 打软删除标记
        let mut active: wallpaper::ActiveModel = model.into();
        active.deleted_at = Set(Some(now.clone()));
        active.updated_at = Set(now.clone());
        active
            .update(&txn)
            .await
            .context("Failed to mark wallpaper as deleted")?;

        // 2. 记录受影响收藏夹，并把关联行的 sort_order 置哨兵
        let rows = collection_wallpaper::Entity::find()
            .filter(collection_wallpaper::Column::WallpaperId.eq(*id))
            .all(&txn)
            .await
            .context("Failed to query collection_wallpapers")?;
        for row in rows {
            affected_collections.insert(row.collection_id);
        }

        collection_wallpaper::Entity::update_many()
            .col_expr(
                collection_wallpaper::Column::SortOrder,
                sea_orm::prelude::Expr::value(TRASH_SORT_ORDER),
            )
            .filter(collection_wallpaper::Column::WallpaperId.eq(*id))
            .exec(&txn)
            .await
            .context("Failed to reset sort_order for trashed wallpaper")?;

        // 3. 解除 monitor_configs 引用（桌面需立刻切走）
        monitor_config::Entity::update_many()
            .col_expr(
                monitor_config::Column::WallpaperId,
                sea_orm::prelude::Expr::value(sea_orm::Value::Int(None)),
            )
            .filter(monitor_config::Column::WallpaperId.eq(*id))
            .exec(&txn)
            .await
            .context("Failed to clean up monitor_configs wallpaper_id")?;

        trashed_count += 1;
    }

    // 对受影响收藏夹的剩余可见成员做连续化重排，消除 sort_order 空洞
    for cid in &affected_collections {
        resequence_collection(&txn, *cid).await?;
    }

    txn.commit().await?;

    info!("[Trash] {} wallpapers moved to trash", trashed_count);
    Ok(trashed_count)
}

/// 将某收藏夹内「可见成员」的 sort_order 连续化为 0..n-1
///
/// 哨兵行（sort_order = -1，对应回收站内的壁纸）不参与重排，保持哨兵值。
/// 在同一事务内调用，保证不产生重复值的中间态。
async fn resequence_collection<C>(txn: &C, collection_id: i32) -> Result<()>
where
    C: sea_orm::ConnectionTrait,
{
    let mut rows = collection_wallpaper::Entity::find()
        .filter(collection_wallpaper::Column::CollectionId.eq(collection_id))
        .filter(collection_wallpaper::Column::SortOrder.ne(TRASH_SORT_ORDER))
        .order_by_asc(collection_wallpaper::Column::SortOrder)
        .all(txn)
        .await
        .context("Failed to query collection members for resequencing")?;

    rows.sort_by_key(|r| r.sort_order);

    for (index, row) in rows.iter().enumerate() {
        let new_order = index as i32;
        if row.sort_order == new_order {
            continue;
        }
        collection_wallpaper::Entity::update_many()
            .col_expr(
                collection_wallpaper::Column::SortOrder,
                sea_orm::prelude::Expr::value(new_order),
            )
            .filter(collection_wallpaper::Column::CollectionId.eq(collection_id))
            .filter(collection_wallpaper::Column::WallpaperId.eq(row.wallpaper_id))
            .exec(txn)
            .await
            .context("Failed to resequence sort_order")?;
    }

    Ok(())
}

/// 从回收站恢复（批量；事务保护）
///
/// 清空 `deleted_at`，并把仍存在的收藏夹关联行按 `max(sort_order) + 1`
/// **追加到末尾**——原位置在数据上已不存在（其他成员可能已被重排过），
/// 追加是唯一可预测、可向用户解释的语义。
pub async fn restore_batch(db: &DatabaseConnection, ids: Vec<i32>) -> Result<u64> {
    use sea_orm::TransactionTrait;

    if ids.is_empty() {
        return Ok(0);
    }

    let txn = db.begin().await?;
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let mut restored_count = 0u64;

    for id in &ids {
        let model = wallpaper::Entity::find_by_id(*id)
            .one(&txn)
            .await
            .context("Failed to query wallpaper")?;

        let Some(model) = model else {
            warn!("[Restore] Wallpaper not found: {}", id);
            continue;
        };
        // 仅处理确实在回收站中的记录
        if model.deleted_at.is_none() {
            continue;
        }

        // 1. 清除软删除标记
        let mut active: wallpaper::ActiveModel = model.into();
        active.deleted_at = Set(None);
        active.updated_at = Set(now.clone());
        active
            .update(&txn)
            .await
            .context("Failed to clear deleted_at")?;

        // 2. 关联行按 max+1 追加回收藏夹末尾
        let rows = collection_wallpaper::Entity::find()
            .filter(collection_wallpaper::Column::WallpaperId.eq(*id))
            .all(&txn)
            .await
            .context("Failed to query collection_wallpapers")?;

        for row in rows {
            let max_order = collection_wallpaper::Entity::find()
                .filter(collection_wallpaper::Column::CollectionId.eq(row.collection_id))
                .filter(collection_wallpaper::Column::SortOrder.ne(TRASH_SORT_ORDER))
                .order_by_desc(collection_wallpaper::Column::SortOrder)
                .one(&txn)
                .await
                .context("Failed to query max sort_order")?
                .map(|r| r.sort_order)
                .unwrap_or(-1);

            collection_wallpaper::Entity::update_many()
                .col_expr(
                    collection_wallpaper::Column::SortOrder,
                    sea_orm::prelude::Expr::value(max_order + 1),
                )
                .filter(collection_wallpaper::Column::CollectionId.eq(row.collection_id))
                .filter(collection_wallpaper::Column::WallpaperId.eq(*id))
                .exec(&txn)
                .await
                .context("Failed to append restored wallpaper to collection tail")?;
        }

        restored_count += 1;
    }

    txn.commit().await?;

    info!("[Restore] {} wallpapers restored from trash", restored_count);
    Ok(restored_count)
}

/// 彻底删除壁纸（事务保护 DB 操作，文件删除在事务提交后执行）
///
/// 真正清理数据库记录、关联表与磁盘文件，不可恢复。
/// 由「彻底删除」「清空回收站」「过期自动清理」三处调用。
pub async fn delete_batch(db: &DatabaseConnection, ids: Vec<i32>) -> Result<u64> {
    use sea_orm::TransactionTrait;

    // 第一阶段：在事务内完成所有 DB 操作，同时收集待删除的文件路径
    let mut files_to_delete: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    let mut deleted_count = 0u64;
    let mut affected_collections: std::collections::HashSet<i32> = std::collections::HashSet::new();

    let txn = db.begin().await?;

    for id in &ids {
        // 1. 查找数据库记录
        let model = wallpaper::Entity::find_by_id(*id)
            .one(&txn)
            .await
            .context("Failed to query wallpaper")?;

        let Some(model) = model else {
            warn!("[Delete] Wallpaper not found: {}", id);
            continue;
        };

        // 收集文件路径，事务提交后再删除
        let file_path = PathBuf::from(&model.file_path);
        let thumb_path = model.thumb_path.as_ref().map(PathBuf::from);
        files_to_delete.push((file_path, thumb_path));

        // 2. 清理关联表：collection_wallpapers 中引用该壁纸的记录
        let rows = collection_wallpaper::Entity::find()
            .filter(collection_wallpaper::Column::WallpaperId.eq(*id))
            .all(&txn)
            .await
            .context("Failed to query collection_wallpapers")?;
        for row in rows {
            affected_collections.insert(row.collection_id);
        }

        collection_wallpaper::Entity::delete_many()
            .filter(collection_wallpaper::Column::WallpaperId.eq(*id))
            .exec(&txn)
            .await
            .context("Failed to clean up collection_wallpapers")?;

        // 3. 清理关联表：monitor_configs 中引用该壁纸的字段置空
        monitor_config::Entity::update_many()
            .col_expr(
                monitor_config::Column::WallpaperId,
                sea_orm::prelude::Expr::value(sea_orm::Value::Int(None)),
            )
            .filter(monitor_config::Column::WallpaperId.eq(*id))
            .exec(&txn)
            .await
            .context("Failed to clean up monitor_configs wallpaper_id")?;

        // 4. 删除数据库记录
        wallpaper::Entity::delete_by_id(*id)
            .exec(&txn)
            .await
            .context("Failed to delete wallpaper from database")?;

        deleted_count += 1;
    }

    // 关联行被移除后，补一次连续化重排，保证游标序列无空洞
    for cid in &affected_collections {
        resequence_collection(&txn, *cid).await?;
    }

    txn.commit().await?;

    // 第二阶段：事务提交成功后，删除物理文件（best-effort，失败仅打印警告）
    for (file_path, thumb_path) in &files_to_delete {
        if file_path.exists() {
            if let Err(e) = std::fs::remove_file(file_path) {
                warn!("[Delete] Failed to remove file {:?}: {}", file_path, e);
            }
        }
        if let Some(ref tp) = thumb_path {
            if tp.exists() {
                if let Err(e) = std::fs::remove_file(tp) {
                    warn!("[Delete] Failed to remove thumbnail {:?}: {}", tp, e);
                }
            }
        }
    }

    info!("[Delete] {} wallpapers permanently deleted", deleted_count);
    Ok(deleted_count)
}

/// 清空回收站：彻底删除回收站内的全部壁纸
pub async fn empty_trash(db: &DatabaseConnection) -> Result<u64> {
    let ids: Vec<i32> = trashed()
        .all(db)
        .await
        .context("Failed to list trashed wallpapers")?
        .into_iter()
        .map(|w| w.id)
        .collect();

    if ids.is_empty() {
        return Ok(0);
    }

    delete_batch(db, ids).await
}

/// 清理超过保留期的回收站壁纸
///
/// `retention_days` 为保留天数；移入时间早于 `now - retention_days` 的记录
/// 会被彻底删除。由应用启动时的一次性扫描调用，不引入常驻定时任务。
pub async fn purge_expired(db: &DatabaseConnection, retention_days: i64) -> Result<u64> {
    if retention_days <= 0 {
        return Ok(0);
    }

    let cutoff = chrono::Local::now() - chrono::Duration::days(retention_days);
    let cutoff_str = cutoff.format("%Y-%m-%d %H:%M:%S").to_string();

    // deleted_at 为 `%Y-%m-%d %H:%M:%S` 定长格式，字典序与时间序一致，可直接比较
    let ids: Vec<i32> = trashed()
        .filter(wallpaper::Column::DeletedAt.lt(cutoff_str.clone()))
        .all(db)
        .await
        .context("Failed to list expired trashed wallpapers")?
        .into_iter()
        .map(|w| w.id)
        .collect();

    if ids.is_empty() {
        return Ok(0);
    }

    info!(
        "[Trash] Purging {} wallpapers older than {}",
        ids.len(),
        cutoff_str
    );
    delete_batch(db, ids).await
}