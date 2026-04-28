use anyhow::{Context, Result};
use image::GenericImageView;
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

/// 导入单个壁纸文件
///
/// 图片/GIF 在导入时由 image crate 生成缩略图；
/// 视频缩略图由前端 canvas 抽帧后通过 `save_video_thumbnail` 单独写入。
pub async fn import_single(
    db: &DatabaseConnection,
    source_path: &str,
    wallpapers_dir: &Path,
    thumbnails_dir: &Path,
) -> Result<wallpaper::Model> {
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
    let file_type = detect_file_type(ext).ok_or_else(|| {
        anyhow::anyhow!("Unsupported file type: .{}", ext)
    })?;

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
    let dest_path_str = dest_path.to_string_lossy().to_string();

    // 6. 生成缩略图：仅图片/GIF 在此生成，视频由前端 canvas 抽帧后单独写入
    let thumb_path_str = if file_type == "image" || file_type == "gif" {
        ensure_dir(thumbnails_dir)?;
        let thumb_name = format!(
            "{}.webp",
            dest_path.file_stem().unwrap().to_string_lossy(),
        );
        let thumb_path = thumbnails_dir.join(&thumb_name);
        match generate_static_thumbnail(&dest_path, &thumb_path) {
            Ok(()) => {
                println!("[Thumbnail] Generated: {:?}", thumb_path);
                Some(thumb_path.to_string_lossy().to_string())
            }
            Err(e) => {
                eprintln!("[WARN] Thumbnail generation failed for {}: {}", original_name, e);
                None
            }
        }
    } else {
        // 视频：thumb_path 暂为 None，等待前端回传
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

    // 8. 获取当前时间
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    // 9. 写入数据库
    let active_model = wallpaper::ActiveModel {
        name: Set(original_name),
        r#type: Set(file_type.to_string()),
        file_path: Set(dest_path_str),
        thumb_path: Set(thumb_path_str),
        width: Set(width),
        height: Set(height),
        duration: Set(None),
        file_size: Set(Some(file_size)),
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

    println!("[Import] {} -> {}", source_path, model.file_path);

    Ok(model)
}

/// 批量导入壁纸
pub async fn import_batch(
    db: &DatabaseConnection,
    source_paths: Vec<String>,
    wallpapers_dir: &Path,
    thumbnails_dir: &Path,
) -> Result<Vec<wallpaper::Model>> {
    let mut results = Vec::new();
    let mut errors = Vec::new();

    for path in &source_paths {
        match import_single(db, path, wallpapers_dir, thumbnails_dir).await {
            Ok(model) => results.push(model),
            Err(e) => {
                eprintln!("[Import Error] {}: {}", path, e);
                errors.push(format!("{}: {}", path, e));
            }
        }
    }

    if results.is_empty() && !errors.is_empty() {
        anyhow::bail!("All imports failed: {}", errors.join("; "));
    }

    Ok(results)
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
        .unwrap_or("unknown");
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

    println!("[VideoThumbnail] Saved: {}", thumb_path_str);
    Ok(thumb_path_str)
}

/// 批量删除壁纸（删文件 + 删缩略图 + 删数据库记录）
pub async fn delete_batch(db: &DatabaseConnection, ids: Vec<i32>) -> Result<u64> {
    let mut deleted_count = 0u64;

    for id in &ids {
        // 1. 查找数据库记录
        let model = wallpaper::Entity::find_by_id(*id)
            .one(db)
            .await
            .context("Failed to query wallpaper")?;

        let Some(model) = model else {
            eprintln!("[Delete] Wallpaper not found: {}", id);
            continue;
        };

        // 2. 删除壁纸文件
        let file_path = Path::new(&model.file_path);
        if file_path.exists() {
            if let Err(e) = std::fs::remove_file(file_path) {
                eprintln!("[Delete] Failed to remove file {}: {}", model.file_path, e);
            }
        }

        // 3. 删除缩略图
        if let Some(ref thumb) = model.thumb_path {
            let thumb_path = Path::new(thumb);
            if thumb_path.exists() {
                if let Err(e) = std::fs::remove_file(thumb_path) {
                    eprintln!("[Delete] Failed to remove thumbnail {}: {}", thumb, e);
                }
            }
        }

        // 4. 清理关联表：collection_wallpapers 中引用该壁纸的记录
        collection_wallpaper::Entity::delete_many()
            .filter(collection_wallpaper::Column::WallpaperId.eq(*id))
            .exec(db)
            .await
            .context("Failed to clean up collection_wallpapers")?;

        // 5. 清理关联表：monitor_configs 中引用该壁纸的字段置空
        monitor_config::Entity::update_many()
            .col_expr(
                monitor_config::Column::WallpaperId,
                sea_orm::prelude::Expr::value(sea_orm::Value::Int(None)),
            )
            .filter(monitor_config::Column::WallpaperId.eq(*id))
            .exec(db)
            .await
            .context("Failed to clean up monitor_configs wallpaper_id")?;

        // 6. 删除数据库记录
        wallpaper::Entity::delete_by_id(*id)
            .exec(db)
            .await
            .context("Failed to delete wallpaper from database")?;

        println!("[Delete] Wallpaper {} deleted", id);
        deleted_count += 1;
    }

    Ok(deleted_count)
}