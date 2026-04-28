use anyhow::{Context, Result};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use log::{info, warn};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use crate::utils::progress_io::{ByteProgressFn, ProgressCounter, ProgressWriter};

/// 导出备份：将 wallpapers/ + thumbnails/ + app.db 打包为 zip
///
/// 进度回调为字节级精度（已写入字节数 / 总字节数），
/// 传入 `None` 时不追踪进度，零额外开销。
pub fn export_backup(
    app_data_dir: &Path,
    output_path: &Path,
    on_progress: Option<ByteProgressFn>,
) -> Result<()> {
    let total = get_data_size(app_data_dir);

    let file = File::create(output_path).context("Failed to create backup file")?;
    let counter = ProgressCounter::new(total, on_progress);
    let writer = ProgressWriter::new(file, counter);
    let mut zip = ZipWriter::new(writer);

    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // 打包目录
    let dirs_to_pack = ["wallpapers", "thumbnails"];
    for dir_name in &dirs_to_pack {
        let dir_path = app_data_dir.join(dir_name);
        if !dir_path.exists() {
            continue;
        }
        for entry in WalkDir::new(&dir_path).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            let relative = path.strip_prefix(app_data_dir).unwrap_or(path);
            let name = relative.to_string_lossy().to_string();

            if path.is_file() {
                zip.start_file(&name, options)
                    .context(format!("Failed to start zip entry: {}", name))?;
                let mut f = File::open(path)
                    .context(format!("Failed to open file: {}", path.display()))?;
                let mut buf = Vec::new();
                f.read_to_end(&mut buf)?;
                zip.write_all(&buf)?;
            } else if path.is_dir() && path != app_data_dir {
                zip.add_directory(&name, options)
                    .context(format!("Failed to add directory: {}", name))?;
            }
        }
    }

    // 打包 app.db
    let db_path = app_data_dir.join("app.db");
    if db_path.exists() {
        zip.start_file("app.db", options)
            .context("Failed to start zip entry: app.db")?;
        let mut f = File::open(&db_path).context("Failed to open app.db")?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        zip.write_all(&buf)?;
    }

    zip.finish().context("Failed to finalize zip")?;

    info!("[Backup] Exported to: {}", output_path.display());
    Ok(())
}

/// 导入备份：解压 zip 到 app_data_dir，覆盖已有文件
///
/// 进度回调为字节级精度（已解压写出字节数 / 解压后总字节数），
/// 多个文件共享同一个 `ProgressCounter`，传入 `None` 时不追踪进度。
pub fn import_backup(
    app_data_dir: &Path,
    zip_path: &Path,
    on_progress: Option<ByteProgressFn>,
) -> Result<u64> {
    let file = File::open(zip_path).context("Failed to open backup file")?;
    let mut archive = ZipArchive::new(file).context("Failed to read zip archive")?;

    // 计算解压后总字节数（所有文件条目的 size 之和）
    let mut total = 0u64;
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            if !entry.is_dir() {
                total += entry.size();
            }
        }
    }

    let counter = ProgressCounter::new(total, on_progress);
    let mut count = 0u64;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).context("Failed to read zip entry")?;
        let name = entry.name().to_string();

        // 安全检查：防止 zip slip 攻击
        if name.contains("..") {
            warn!("[Backup] Skipping suspicious entry: {}", name);
            continue;
        }

        let out_path = app_data_dir.join(&name);

        if entry.is_dir() {
            fs::create_dir_all(&out_path)
                .context(format!("Failed to create directory: {}", out_path.display()))?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }

            let out_file = File::create(&out_path)
                .context(format!("Failed to create file: {}", out_path.display()))?;

            // 共享同一个 counter，多文件解压的字节进度自动累加
            let mut progress_out = ProgressWriter::new(out_file, counter.clone());
            std::io::copy(&mut entry, &mut progress_out)?;
            count += 1;
        }
    }

    info!("[Backup] Imported {} files from: {}", count, zip_path.display());
    Ok(count)
}

/// 获取 app_data_dir 的总大小（字节）
pub fn get_data_size(app_data_dir: &Path) -> u64 {
    let mut total = 0u64;
    for entry in WalkDir::new(app_data_dir).into_iter().filter_map(|e| e.ok()) {
        if entry.path().is_file() {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    total
}