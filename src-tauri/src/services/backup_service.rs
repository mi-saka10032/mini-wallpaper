use anyhow::{bail, Context, Result};
use log::{info, warn};
use sea_orm::{ConnectionTrait, DatabaseConnection};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

/// 单次导入允许的 ZIP 条目数。壁纸库远低于此值，但能挡住海量空条目炸弹。
const MAX_ZIP_ENTRIES: usize = 100_000;
/// 声明体积不超过该值时不检查压缩比，避免小文本的正常压缩被误伤。
const COMPRESSION_RATIO_FLOOR: u64 = 1024 * 1024;
/// 超过 floor 后，解压体积不得超过压缩体积的该倍数。
const MAX_COMPRESSION_RATIO: u64 = 64;

use crate::utils::progress_io::{ByteProgressFn, ProgressCounter, ProgressWriter};

/// 用 SQLite `VACUUM INTO` 生成独立的一致性快照。
///
/// 直接复制 `app.db` 会漏掉尚未 checkpoint 的 WAL 事务。快照文件包含调用时
/// 已提交的全部数据，并且不附带 WAL，可以被安全地放进备份包。
pub async fn snapshot_database(db: &DatabaseConnection, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).context("Failed to create database snapshot directory")?;
    }
    if destination.exists() {
        fs::remove_file(destination).context("Failed to remove previous database snapshot")?;
    }

    let path = destination
        .to_str()
        .context("Database snapshot path is not valid UTF-8")?;
    if path.contains('\0') {
        bail!("Database snapshot path is invalid");
    }
    let escaped = path.replace('\'', "''");
    db.execute_unprepared(&format!("VACUUM INTO '{escaped}'"))
        .await
        .context("Failed to snapshot database")?;
    Ok(())
}

/// 导出备份：将 wallpapers/ + thumbnails/ + 数据库快照打包为 zip
///
/// `database_snapshot` 必须是已经脱离 WAL 的完整数据库文件，会以 `app.db`
/// 的名字写入压缩包。进度回调为字节级精度，传入 `None` 时不追踪进度。
pub fn export_backup(
    app_data_dir: &Path,
    database_snapshot: &Path,
    output_path: &Path,
    on_progress: Option<ByteProgressFn>,
) -> Result<()> {
    let total = packed_payload_size(app_data_dir, database_snapshot);

    let file = File::create(output_path).context("Failed to create backup file")?;
    let counter = ProgressCounter::new(total, on_progress);
    let writer = ProgressWriter::new(file, counter);
    let mut zip = ZipWriter::new(writer);

    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

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
                let mut f =
                    File::open(path).context(format!("Failed to open file: {}", path.display()))?;
                let mut buf = Vec::new();
                f.read_to_end(&mut buf)?;
                zip.write_all(&buf)?;
            } else if path.is_dir() && path != app_data_dir {
                zip.add_directory(&name, options)
                    .context(format!("Failed to add directory: {}", name))?;
            }
        }
    }

    // 打包一致性快照，压缩包内固定命名为 app.db。
    if database_snapshot.exists() {
        zip.start_file("app.db", options)
            .context("Failed to start zip entry: app.db")?;
        let mut f = File::open(database_snapshot).context("Failed to open database snapshot")?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        zip.write_all(&buf)?;
    }

    zip.finish().context("Failed to finalize zip")?;

    info!("[Backup] Exported to: {}", output_path.display());
    Ok(())
}

fn packed_payload_size(app_data_dir: &Path, database_snapshot: &Path) -> u64 {
    let mut total = 0u64;
    for dir_name in ["wallpapers", "thumbnails"] {
        let dir_path = app_data_dir.join(dir_name);
        if dir_path.exists() {
            total = total.saturating_add(get_data_size(&dir_path));
        }
    }
    if database_snapshot.exists() {
        total = total.saturating_add(
            fs::metadata(database_snapshot)
                .map(|m| m.len())
                .unwrap_or(0),
        );
    }
    total
}

/// 把已校验的临时导入目录安装到应用目录。
///
/// 壁纸和缩略图按条目覆盖，不删除备份里没有的本地文件。`app.db` 最后替换，
/// 并删除旧的 WAL/SHM，避免重启时把旧日志回放到新数据库上。
///
/// 调用方必须先关闭正在使用该数据库的连接。本函数不关闭连接。
pub fn install_staged_backup(staging: &Path, app_data_dir: &Path) -> Result<()> {
    copy_staged_files(staging, app_data_dir)?;
    let staged_db = staging.join("app.db");
    if staged_db.exists() {
        replace_database_file(&app_data_dir.join("app.db"), &staged_db)?;
    }
    Ok(())
}

fn copy_staged_files(staging: &Path, app_data_dir: &Path) -> Result<()> {
    for entry in WalkDir::new(staging)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        let path = entry.path();
        let relative = path.strip_prefix(staging).unwrap_or(path);
        if relative.as_os_str().is_empty() || relative == Path::new("app.db") {
            continue;
        }
        let target = app_data_dir.join(relative);
        if path.is_dir() {
            fs::create_dir_all(&target)
                .context(format!("Failed to create directory: {}", target.display()))?;
        } else if path.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(path, &target)
                .context(format!("Failed to restore file: {}", target.display()))?;
        }
    }
    Ok(())
}

fn replace_database_file(live: &Path, staged: &Path) -> Result<()> {
    let parent = live
        .parent()
        .context("Database path has no parent directory")?;
    let incoming = parent.join("app.db.incoming");
    let previous = parent.join("app.db.previous");
    // 上次中断可能留下半成品。数据库仍在时，previous 只是未能删掉的旧副本。
    if previous.exists() {
        if live.exists() {
            fs::remove_file(&previous).context("Failed to remove stale database backup")?;
        } else {
            fs::rename(&previous, live).context("Failed to restore interrupted database")?;
        }
    }
    if incoming.exists() {
        fs::remove_file(&incoming).context("Failed to remove stale incoming database")?;
    }
    fs::copy(staged, &incoming).context("Failed to stage restored database")?;

    if live.exists() {
        fs::rename(live, &previous).context("Failed to move the current database aside")?;
    }
    if let Err(error) = fs::rename(&incoming, live) {
        if previous.exists() {
            let _ = fs::rename(&previous, live);
        }
        return Err(error).context("Failed to replace app.db");
    }

    for sidecar in ["app.db-wal", "app.db-shm", "app.db.previous"] {
        let path = parent.join(sidecar);
        if path.exists() {
            fs::remove_file(&path).context(format!(
                "Failed to remove old database file: {}",
                path.display()
            ))?;
        }
    }
    Ok(())
}

/// 导入限额。生产路径使用 [`IMPORT_LIMITS`]，测试可收紧以构造小样本。
#[derive(Clone, Copy)]
struct ImportLimits {
    max_entries: usize,
    ratio_floor: u64,
    max_ratio: u64,
}

const IMPORT_LIMITS: ImportLimits = ImportLimits {
    max_entries: MAX_ZIP_ENTRIES,
    ratio_floor: COMPRESSION_RATIO_FLOOR,
    max_ratio: MAX_COMPRESSION_RATIO,
};

/// 导入备份：解压 zip 到 app_data_dir，覆盖已有文件
///
/// 进度回调为字节级精度（已解压写出字节数 / 解压后总字节数），
/// 多个文件共享同一个 `ProgressCounter`，传入 `None` 时不追踪进度。
///
/// 写入前会整包校验：条目必须落在应用目录内，且条目数和压缩比不超过限额。
/// 任一条目非法即拒绝导入，避免恶意归档先覆盖一部分合法文件。
pub fn import_backup(
    app_data_dir: &Path,
    zip_path: &Path,
    on_progress: Option<ByteProgressFn>,
) -> Result<u64> {
    import_backup_with_limits(app_data_dir, zip_path, on_progress, IMPORT_LIMITS)
}

fn import_backup_with_limits(
    app_data_dir: &Path,
    zip_path: &Path,
    on_progress: Option<ByteProgressFn>,
    limits: ImportLimits,
) -> Result<u64> {
    let app_data_dir = app_data_dir
        .canonicalize()
        .context("Backup directory does not exist")?;
    let file = File::open(zip_path).context("Failed to open backup file")?;
    let mut archive = ZipArchive::new(file).context("Failed to read zip archive")?;

    if archive.len() > limits.max_entries {
        bail!(
            "Backup contains {} entries, exceeding the limit of {}",
            archive.len(),
            limits.max_entries
        );
    }

    // 先校验全部条目，通过后才允许写盘。
    let mut total = 0u64;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).context("Failed to read zip entry")?;
        validate_entry(&app_data_dir, &entry, limits)?;
        if !entry.is_dir() {
            total = total.saturating_add(entry.size());
        }
    }

    let counter = ProgressCounter::new(total, on_progress);
    let mut count = 0u64;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).context("Failed to read zip entry")?;
        let out_path = entry_output_path(&app_data_dir, &entry)?;

        if entry.is_dir() {
            fs::create_dir_all(&out_path).context(format!(
                "Failed to create directory: {}",
                out_path.display()
            ))?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // 先写入临时文件，完整读完后再替换目标，避免超限时截断已有文件。
        let temporary = temporary_path(&out_path);
        let out_file = File::create(&temporary)
            .context(format!("Failed to create file: {}", temporary.display()))?;
        let mut progress_out = ProgressWriter::new(out_file, counter.clone());
        let declared_size = entry.size();
        if let Err(error) = copy_capped(&mut entry, &mut progress_out, declared_size) {
            drop(progress_out);
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        drop(progress_out);
        fs::rename(&temporary, &out_path).context(format!(
            "Failed to replace backup file: {}",
            out_path.display()
        ))?;
        count += 1;
    }

    info!(
        "[Backup] Imported {} files from: {}",
        count,
        zip_path.display()
    );
    Ok(count)
}

fn validate_entry(
    app_data_dir: &Path,
    entry: &zip::read::ZipFile<'_>,
    limits: ImportLimits,
) -> Result<()> {
    entry_output_path(app_data_dir, entry)?;
    if entry.is_dir() {
        return Ok(());
    }
    if compression_ratio_exceeded(entry.size(), entry.compressed_size(), limits) {
        warn!(
            "[Backup] Rejecting entry with excessive compression ratio: {}",
            entry.name()
        );
        bail!(
            "Backup entry '{}' expands beyond the allowed compression ratio",
            entry.name()
        );
    }
    Ok(())
}

/// 把 ZIP 内的相对路径解析到应用目录内。绝对路径、空路径和逃逸 `..` 都拒绝。
fn entry_output_path(app_data_dir: &Path, entry: &zip::read::ZipFile<'_>) -> Result<PathBuf> {
    let Some(relative) = entry.enclosed_name() else {
        warn!("[Backup] Rejecting unsafe entry: {}", entry.name());
        bail!(
            "Backup entry '{}' escapes the backup directory",
            entry.name()
        );
    };
    if relative.as_os_str().is_empty() {
        bail!("Backup entry path is empty");
    }

    let mut out = app_data_dir.to_path_buf();
    for component in relative.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() || !out.starts_with(app_data_dir) {
                    bail!(
                        "Backup entry '{}' escapes the backup directory",
                        entry.name()
                    );
                }
            }
            Component::Prefix(_) | Component::RootDir => {
                bail!("Backup entry '{}' is absolute", entry.name());
            }
        }
    }

    if out == app_data_dir || !out.starts_with(app_data_dir) {
        bail!(
            "Backup entry '{}' escapes the backup directory",
            entry.name()
        );
    }
    Ok(out)
}

fn compression_ratio_exceeded(uncompressed: u64, compressed: u64, limits: ImportLimits) -> bool {
    if uncompressed <= limits.ratio_floor {
        return false;
    }
    uncompressed / compressed.max(1) > limits.max_ratio
}

fn temporary_path(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_os_string();
    name.push(".partial");
    PathBuf::from(name)
}

/// 最多复制 `limit` 字节。超出声明体积时停止，不把多余字节写入目标。
fn copy_capped(reader: &mut impl Read, writer: &mut impl Write, limit: u64) -> Result<u64> {
    let mut buffer = [0u8; 64 * 1024];
    let mut written = 0u64;
    loop {
        let read = reader
            .read(&mut buffer)
            .context("Failed to read zip entry")?;
        if read == 0 {
            break;
        }
        let read = read as u64;
        if written.saturating_add(read) > limit {
            bail!("Backup entry expanded beyond its declared size");
        }
        writer
            .write_all(&buffer[..read as usize])
            .context("Failed to write backup file")?;
        written += read;
    }
    Ok(written)
}

/// 获取 app_data_dir 的总大小（字节）
pub fn get_data_size(app_data_dir: &Path) -> u64 {
    let mut total = 0u64;
    for entry in WalkDir::new(app_data_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.path().is_file() {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicU64, Ordering};
    use zip::CompressionMethod;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "mini-wallpaper-backup-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn permissive_limits() -> ImportLimits {
        ImportLimits {
            max_entries: 10,
            ratio_floor: u64::MAX,
            max_ratio: u64::MAX,
        }
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])], method: CompressionMethod) {
        let file = File::create(path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(method);
        for (name, data) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn imports_entries_inside_the_backup_directory() {
        let root = TempDir::new();
        let app = root.0.join("app");
        fs::create_dir(&app).unwrap();
        let zip_path = root.0.join("ok.zip");
        write_zip(
            &zip_path,
            &[("wallpapers/a.txt", b"ok"), ("app.db", b"db")],
            CompressionMethod::Stored,
        );

        let count = import_backup_with_limits(&app, &zip_path, None, permissive_limits()).unwrap();

        assert_eq!(count, 2);
        assert_eq!(fs::read(app.join("wallpapers/a.txt")).unwrap(), b"ok");
        assert_eq!(fs::read(app.join("app.db")).unwrap(), b"db");
    }

    #[test]
    fn rejects_parent_and_absolute_entries_before_writing() {
        let root = TempDir::new();
        let app = root.0.join("app");
        fs::create_dir(&app).unwrap();
        let outside = root.0.join("outside.txt");
        let escaped = root.0.join("escaped.txt");
        let cases = [
            ("parent.zip", "../outside.txt"),
            ("nested-parent.zip", "foo/../../outside.txt"),
            ("absolute.zip", escaped.to_str().unwrap()),
        ];

        for (file_name, entry_name) in cases {
            let zip_path = root.0.join(file_name);
            write_zip(
                &zip_path,
                &[(entry_name, b"pwned")],
                CompressionMethod::Stored,
            );

            let error =
                import_backup_with_limits(&app, &zip_path, None, permissive_limits()).unwrap_err();

            assert!(
                error.to_string().contains("escapes the backup directory"),
                "{entry_name}: {error}"
            );
        }

        assert!(!outside.exists());
        assert!(!escaped.exists());
        assert!(fs::read_dir(&app).unwrap().next().is_none());
    }

    #[test]
    fn rejects_too_many_entries() {
        let root = TempDir::new();
        let app = root.0.join("app");
        fs::create_dir(&app).unwrap();
        let zip_path = root.0.join("many.zip");
        write_zip(
            &zip_path,
            &[("a.txt", b"a"), ("b.txt", b"b"), ("c.txt", b"c")],
            CompressionMethod::Stored,
        );
        let limits = ImportLimits {
            max_entries: 2,
            ..permissive_limits()
        };

        let error = import_backup_with_limits(&app, &zip_path, None, limits).unwrap_err();

        assert!(error.to_string().contains("exceeding the limit"));
        assert!(fs::read_dir(&app).unwrap().next().is_none());
    }

    #[test]
    fn rejects_excessive_compression_ratio() {
        let root = TempDir::new();
        let app = root.0.join("app");
        fs::create_dir(&app).unwrap();
        let zip_path = root.0.join("bomb.zip");
        let payload = vec![0u8; 256 * 1024];
        write_zip(
            &zip_path,
            &[("wallpapers/bomb.bin", &payload)],
            CompressionMethod::Deflated,
        );
        let limits = ImportLimits {
            max_entries: 10,
            ratio_floor: 64 * 1024,
            max_ratio: 4,
        };

        let error = import_backup_with_limits(&app, &zip_path, None, limits).unwrap_err();

        assert!(error.to_string().contains("compression ratio"));
        assert!(!app.join("wallpapers/bomb.bin").exists());
    }

    #[test]
    fn copy_capped_stops_at_declared_size() {
        let mut input = Cursor::new(vec![1u8; 10]);
        let mut output = Vec::new();

        let error = copy_capped(&mut input, &mut output, 4).unwrap_err();

        assert!(error.to_string().contains("declared size"));
        assert!(output.len() <= 4);
    }

    #[test]
    fn export_then_import_round_trip() {
        let root = TempDir::new();
        let source = root.0.join("source");
        let target = root.0.join("target");
        fs::create_dir_all(source.join("wallpapers")).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(source.join("wallpapers/a.txt"), b"ok").unwrap();
        fs::write(source.join("snapshot.db"), b"db").unwrap();
        let zip_path = root.0.join("backup.zip");

        export_backup(&source, &source.join("snapshot.db"), &zip_path, None).unwrap();
        let count = import_backup(&target, &zip_path, None).unwrap();

        assert_eq!(count, 2);
        assert_eq!(fs::read(target.join("wallpapers/a.txt")).unwrap(), b"ok");
        assert_eq!(fs::read(target.join("app.db")).unwrap(), b"db");
    }

    #[test]
    fn export_packs_snapshot_instead_of_the_live_database_file() {
        let root = TempDir::new();
        let source = root.0.join("source");
        let target = root.0.join("target");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(source.join("app.db"), b"STALE").unwrap();
        fs::write(source.join("snapshot.db"), b"FRESH").unwrap();
        let zip_path = root.0.join("backup.zip");

        export_backup(&source, &source.join("snapshot.db"), &zip_path, None).unwrap();
        import_backup(&target, &zip_path, None).unwrap();

        assert_eq!(fs::read(target.join("app.db")).unwrap(), b"FRESH");
    }

    #[test]
    fn import_does_not_replace_live_database_until_install() {
        let root = TempDir::new();
        let live = root.0.join("live");
        let staging = root.0.join("staging");
        fs::create_dir_all(live.join("wallpapers")).unwrap();
        fs::create_dir(&staging).unwrap();
        fs::write(live.join("app.db"), b"OLD").unwrap();
        fs::write(live.join("app.db-wal"), b"WAL").unwrap();
        fs::write(live.join("wallpapers/keep.txt"), b"keep").unwrap();
        let zip_path = root.0.join("backup.zip");
        write_zip(
            &zip_path,
            &[("wallpapers/a.txt", b"new"), ("app.db", b"NEWDB")],
            CompressionMethod::Stored,
        );

        import_backup(&staging, &zip_path, None).unwrap();

        assert_eq!(fs::read(live.join("app.db")).unwrap(), b"OLD");
        assert_eq!(fs::read(live.join("app.db-wal")).unwrap(), b"WAL");
        assert!(!live.join("wallpapers/a.txt").exists());

        install_staged_backup(&staging, &live).unwrap();

        assert_eq!(fs::read(live.join("app.db")).unwrap(), b"NEWDB");
        assert!(!live.join("app.db-wal").exists());
        assert_eq!(fs::read(live.join("wallpapers/a.txt")).unwrap(), b"new");
        assert_eq!(fs::read(live.join("wallpapers/keep.txt")).unwrap(), b"keep");
    }

    #[tokio::test]
    async fn snapshot_includes_uncheckpointed_wal_commits() {
        use sea_orm::{Database, DatabaseBackend, Statement};

        let root = TempDir::new();
        let db_path = root.0.join("app.db");
        let db = Database::connect(format!("sqlite:{}?mode=rwc", db_path.display()))
            .await
            .unwrap();
        db.execute_unprepared("PRAGMA journal_mode=WAL;")
            .await
            .unwrap();
        db.execute_unprepared("PRAGMA wal_autocheckpoint=0;")
            .await
            .unwrap();
        db.execute_unprepared("CREATE TABLE items(value TEXT);")
            .await
            .unwrap();
        db.execute_unprepared("INSERT INTO items VALUES ('uncheckpointed');")
            .await
            .unwrap();
        assert!(root.0.join("app.db-wal").exists());

        let naive = root.0.join("naive.db");
        fs::copy(&db_path, &naive).unwrap();
        let naive_db = Database::connect(format!("sqlite:{}?mode=rw", naive.display()))
            .await
            .unwrap();
        let naive_rows = naive_db
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT value FROM items",
            ))
            .await;
        assert!(
            naive_rows.is_err(),
            "copying only app.db must miss the WAL commit"
        );

        let snapshot = root.0.join("snapshot.db");
        snapshot_database(&db, &snapshot).await.unwrap();
        let snapshot_db = Database::connect(format!("sqlite:{}?mode=ro", snapshot.display()))
            .await
            .unwrap();
        let rows = snapshot_db
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT value FROM items",
            ))
            .await
            .unwrap();
        let value: String = rows[0].try_get("", "value").unwrap();
        assert_eq!(value, "uncheckpointed");
    }
}
