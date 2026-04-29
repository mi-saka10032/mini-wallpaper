use anyhow::{Context, Result};
use futures::stream::{self, StreamExt};
use image::GenericImageView;
use log::{info, warn};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::entities::{collection_wallpaper, monitor_config, wallpaper};

/// 支持的图片扩展名
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "bmp", "webp"];

/// 支持的视频扩展名
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "webm", "mkv", "avi", "mov"];

/// 支持的 GIF 扩展名
const GIF_EXTENSIONS: &[&str] = &["gif"];

/// 获取所有壁纸
pub async fn get_all(db: &DatabaseConnection) -> Result<Vec<wallpaper::Model>> {
    wallpaper::Entity::find()
        .all(db)
        .await
        .context("Failed to fetch wallpapers")
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
        tags: Set(None),
        is_favorite: Set(0),
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
/// 使用 `buffer_unordered` 控制最大并发数为 3，
/// 每个任务内部通过 `spawn_blocking` 执行文件 I/O，避免阻塞 async runtime。
/// SQLite 写操作由连接池自动排队，无需额外加锁。
const IMPORT_CONCURRENCY: usize = 3;

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
            .buffer_unordered(IMPORT_CONCURRENCY)
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

/// 批量删除壁纸（事务保护 DB 操作，文件删除在事务提交后执行）
pub async fn delete_batch(db: &DatabaseConnection, ids: Vec<i32>) -> Result<u64> {
    use sea_orm::TransactionTrait;

    // 第一阶段：在事务内完成所有 DB 操作，同时收集待删除的文件路径
    let mut files_to_delete: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    let mut deleted_count = 0u64;

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

    info!("[Delete] {} wallpapers deleted", deleted_count);
    Ok(deleted_count)
}