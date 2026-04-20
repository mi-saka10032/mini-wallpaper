use anyhow::{Context, Result};
use image::GenericImageView;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::entities::wallpaper;
use crate::utils::ffmpeg;

/// 支持的图片扩展名
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "bmp", "webp"];

/// 支持的视频扩展名
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "webm", "mkv", "avi", "mov"];

/// 支持的 GIF 扩展名
const GIF_EXTENSIONS: &[&str] = &["gif"];

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

/// 生成图片缩略图（image crate）
fn generate_image_thumbnail(source: &Path, thumb_path: &Path) -> Result<()> {
    let img = image::open(source).context("Failed to open image")?;
    let thumbnail = img.thumbnail(400, 400);
    thumbnail
        .save(thumb_path)
        .context("Failed to save thumbnail")?;
    Ok(())
}

/// 按文件类型分发缩略图生成
///
/// - image: image crate 缩放
/// - gif: image crate 解码第一帧
/// - video: ffmpeg 抽帧
fn generate_thumbnail(
    file_type: &str,
    source: &Path,
    thumb_path: &Path,
    ffmpeg_path: &str,
) -> Result<()> {
    match file_type {
        "image" => generate_image_thumbnail(source, thumb_path),
        "gif" => ffmpeg::generate_gif_thumbnail(source, thumb_path),
        "video" => ffmpeg::generate_video_thumbnail(ffmpeg_path, source, thumb_path),
        _ => anyhow::bail!("Unsupported file type for thumbnail: {}", file_type),
    }
}

/// 获取图片尺寸
fn get_image_dimensions(path: &Path) -> Option<(u32, u32)> {
    image::open(path).ok().map(|img| img.dimensions())
}

/// 导入单个壁纸文件
pub async fn import_single(
    db: &DatabaseConnection,
    source_path: &str,
    wallpapers_dir: &Path,
    thumbnails_dir: &Path,
    ffmpeg_path: &str,
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

    // 6. 生成缩略图（按文件类型分发：image→image crate, gif→image crate 第一帧, video→ffmpeg）
    ensure_dir(thumbnails_dir)?;
    // 视频缩略图输出 jpg（ffmpeg 输出），图片/GIF 输出 webp（image crate）
    let thumb_ext = if file_type == "video" { "jpg" } else { "webp" };
    let thumb_name = format!(
        "{}.{}",
        dest_path.file_stem().unwrap().to_string_lossy(),
        thumb_ext
    );
    let thumb_path = thumbnails_dir.join(&thumb_name);
    let thumb_path_str = match generate_thumbnail(file_type, &dest_path, &thumb_path, ffmpeg_path) {
        Ok(()) => {
            println!("[Thumbnail] Generated: {:?}", thumb_path);
            Some(thumb_path.to_string_lossy().to_string())
        }
        Err(e) => {
            eprintln!("[WARN] Thumbnail generation failed for {}: {}", original_name, e);
            None
        }
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
    ffmpeg_path: &str,
) -> Result<Vec<wallpaper::Model>> {
    let mut results = Vec::new();
    let mut errors = Vec::new();

    for path in &source_paths {
        match import_single(db, path, wallpapers_dir, thumbnails_dir, ffmpeg_path).await {
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

        // 4. 删除数据库记录
        wallpaper::Entity::delete_by_id(*id)
            .exec(db)
            .await
            .context("Failed to delete wallpaper from database")?;

        println!("[Delete] Wallpaper {} deleted", id);
        deleted_count += 1;
    }

    Ok(deleted_count)
}
