use std::env;
use std::path::Path;

use super::contracts::{
    BackendSelection, RenderBackend, RendererError, RendererErrorCode, WallpaperKind,
};

// Media Foundation 原生支持的容器扩展名（见微软官方 Supported Media Formats in Media Foundation）。
// 这里声明的是「交给 Native 解码器尝试」，不保证容器内编码一定能解；编码不兼容时
// 渲染线程会回传 DecodeFailed，不回退 WebView。
// 表外格式属于 capability 不支持，Auto 下走 WebView（见 resolve_backend）。
const NATIVE_VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "avi", "wmv", "asf", "3gp", "3g2", "3gp2", "3gpp",
];

pub fn default_backend_selection() -> BackendSelection {
    if cfg!(debug_assertions) {
        return BackendSelection::NativeOnly;
    }

    let forced = matches!(env::var("MINI_WALLPAPER_RENDERER").as_deref(), Ok("native"));
    if forced {
        BackendSelection::NativeOnly
    } else {
        BackendSelection::Auto
    }
}

pub fn resolve_backend(
    kind: WallpaperKind,
    file_path: &Path,
    selection: BackendSelection,
) -> Result<RenderBackend, RendererError> {
    if matches!(kind, WallpaperKind::Image | WallpaperKind::Gif) {
        return Ok(RenderBackend::Native);
    }

    let ext = file_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if NATIVE_VIDEO_EXTENSIONS.contains(&ext.as_str()) {
        return Ok(RenderBackend::Native);
    }

    // M8：WebView 只做「解码前」的 capability fallback。容器表之外的格式在
    // Auto 下交给 wallpaper.html 播放；NativeOnly 下明确报错。
    // 一旦选定 Native，解码失败只回传 DecodeFailed，不二次回退 WebView。
    match selection {
        BackendSelection::Auto => Ok(RenderBackend::WebView),
        BackendSelection::NativeOnly => Err(RendererError::new(
            RendererErrorCode::UnsupportedFormat,
            format!("unsupported video format: .{}", ext),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn path(ext: &str) -> PathBuf {
        PathBuf::from(format!("/tmp/wallpaper.{}", ext))
    }

    #[test]
    fn image_and_gif_always_use_native() {
        for selection in [BackendSelection::NativeOnly, BackendSelection::Auto] {
            assert_eq!(
                resolve_backend(WallpaperKind::Image, &path("webp"), selection),
                Ok(RenderBackend::Native)
            );
            assert_eq!(
                resolve_backend(WallpaperKind::Gif, &path("gif"), selection),
                Ok(RenderBackend::Native)
            );
        }
    }

    #[test]
    fn media_foundation_video_extensions_use_native_in_both_modes() {
        for ext in [
            "mp4", "m4v", "mov", "avi", "wmv", "asf", "3gp", "3g2", "3gp2", "3gpp",
        ] {
            for selection in [BackendSelection::NativeOnly, BackendSelection::Auto] {
                assert_eq!(
                    resolve_backend(WallpaperKind::Video, &path(ext), selection),
                    Ok(RenderBackend::Native),
                    ".{} 应由 Media Foundation 原生解码",
                    ext
                );
            }
        }
    }

    #[test]
    fn unsupported_video_falls_back_only_in_auto() {
        for ext in ["webm", "mkv", "flv"] {
            assert_eq!(
                resolve_backend(WallpaperKind::Video, &path(ext), BackendSelection::Auto),
                Ok(RenderBackend::WebView),
                ".{} 应由 WebView 作为 capability fallback",
                ext
            );
            assert_eq!(
                resolve_backend(
                    WallpaperKind::Video,
                    &path(ext),
                    BackendSelection::NativeOnly
                ),
                Err(RendererError::new(
                    RendererErrorCode::UnsupportedFormat,
                    format!("unsupported video format: .{}", ext)
                )),
                ".{} 在 NativeOnly 下应明确报错",
                ext
            );
        }
    }
}
