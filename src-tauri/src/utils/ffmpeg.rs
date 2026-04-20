// =============================================================================
// TODO: Windows 精简版 FFmpeg 安装步骤
// =============================================================================
//
// 开发阶段（当前）：
//   - macOS: `brew install ffmpeg`
//   - Windows: 下载预编译版并添加到 PATH
//     1. 访问 https://github.com/BtbN/FFmpeg-Builds/releases
//     2. 下载 `ffmpeg-master-latest-win64-lgpl.zip`（LGPL 版，避免 GPL 传染）
//     3. 解压后将 `bin/ffmpeg.exe` 所在目录添加到系统 PATH
//     4. 验证：`ffmpeg -version`
//
// 分发阶段（后续 bundle 精简版）：
//   - 自行编译精简版 FFmpeg（仅保留解码器 + MJPEG 编码 + scale 滤镜）：
//     ```
//     ./configure \
//       --disable-everything \
//       --enable-demuxer=mov,matroska,avi,gif,webm \
//       --enable-decoder=h264,hevc,vp8,vp9,av1,gif,mjpeg,png \
//       --enable-encoder=mjpeg \
//       --enable-muxer=image2 \
//       --enable-protocol=file \
//       --enable-filter=scale \
//       --disable-doc \
//       --disable-ffplay \
//       --disable-ffprobe \
//       --disable-network \
//       --enable-small \
//       --extra-cflags="-Os"
//     ```
//     预估体积：Windows ~8-15MB
//
//   - 编译产物放置位置：
//     编译完成后，将 ffmpeg.exe 放到项目的以下路径：
//     ```
//     src-tauri/bin/ffmpeg.exe      ← Windows 精简版
//     src-tauri/bin/ffmpeg           ← macOS 版（如需 bundle）
//     ```
//     目录结构：
//     ```
//     mini-wallpaper/
//     ├── src-tauri/
//     │   ├── bin/
//     │   │   ├── ffmpeg.exe         ← 编译/下载的精简版（Win）
//     │   │   └── ffmpeg             ← 编译/下载的版本（Mac，可选）
//     │   ├── src/
//     │   ├── Cargo.toml
//     │   └── tauri.conf.json        ← 需添加 bundle.resources 配置
//     ```
//
//   - Bundle 配置（tauri.conf.json）：
//     ```json
//     {
//       "bundle": {
//         "resources": {
//           "bin/ffmpeg.exe": "bin/ffmpeg.exe"
//         }
//       }
//     }
//     ```
//     打包后 ffmpeg.exe 会被复制到应用的 resource_dir/bin/ 下，
//     get_ffmpeg_path() 会自动优先使用该路径。
//
//   - 也可从 https://github.com/BtbN/FFmpeg-Builds/releases 下载完整版，
//     只取 ffmpeg.exe 一个文件（~80-130MB），同样放到 src-tauri/bin/ 目录下。
//
// =============================================================================

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// 获取 ffmpeg 可执行文件路径
///
/// 策略：优先查找 bundle 目录下的精简版，fallback 到系统 PATH
pub fn get_ffmpeg_path(app: &tauri::AppHandle) -> String {
    use tauri::Manager;

    // 优先查找 bundle 版（resource_dir/bin/ffmpeg 或 ffmpeg.exe）
    if let Ok(resource_dir) = app.path().resource_dir() {
        let bin_name = if cfg!(target_os = "windows") {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        };
        let bundled = resource_dir.join("bin").join(bin_name);
        if bundled.exists() {
            return bundled.to_string_lossy().to_string();
        }
    }

    // Fallback: 系统 PATH
    "ffmpeg".to_string()
}

/// 检测 ffmpeg 是否可用
pub fn is_ffmpeg_available(ffmpeg_path: &str) -> bool {
    Command::new(ffmpeg_path)
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// 使用 ffmpeg 生成视频缩略图
///
/// 抽取第 1 秒的关键帧，缩放到 400px 宽度（保持比例），输出为 JPEG
pub fn generate_video_thumbnail(
    ffmpeg_path: &str,
    video_path: &Path,
    output_path: &Path,
) -> Result<()> {
    let output = Command::new(ffmpeg_path)
        .args([
            "-i",
            &video_path.to_string_lossy(),
            "-ss",
            "00:00:01",
            "-frames:v",
            "1",
            "-vf",
            "scale=400:-1",
            "-q:v",
            "2",
            "-y", // 覆盖已存在文件
            &output_path.to_string_lossy(),
        ])
        .output()
        .context("Failed to execute ffmpeg")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);

        // 如果 -ss 1s 失败（视频可能不足 1 秒），尝试抽第 0 帧
        let fallback_output = Command::new(ffmpeg_path)
            .args([
                "-i",
                &video_path.to_string_lossy(),
                "-frames:v",
                "1",
                "-vf",
                "scale=400:-1",
                "-q:v",
                "2",
                "-y",
                &output_path.to_string_lossy(),
            ])
            .output()
            .context("Failed to execute ffmpeg (fallback)")?;

        if !fallback_output.status.success() {
            let fallback_stderr = String::from_utf8_lossy(&fallback_output.stderr);
            anyhow::bail!(
                "ffmpeg failed:\n  first attempt: {}\n  fallback: {}",
                stderr.trim(),
                fallback_stderr.trim()
            );
        }
    }

    // 验证输出文件确实生成了
    if !output_path.exists() {
        anyhow::bail!("ffmpeg completed but output file not found: {:?}", output_path);
    }

    Ok(())
}

/// 使用 image crate 生成 GIF 缩略图（解码第一帧）
pub fn generate_gif_thumbnail(gif_path: &Path, output_path: &Path) -> Result<()> {
    let img = image::open(gif_path).context("Failed to open GIF file")?;
    let thumbnail = img.thumbnail(400, 400);
    thumbnail
        .save(output_path)
        .context("Failed to save GIF thumbnail")?;
    Ok(())
}
