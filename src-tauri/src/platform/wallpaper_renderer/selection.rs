use std::env;
use std::path::Path;

use super::contracts::{
    BackendSelection, RenderBackend, RendererError, RendererErrorCode, WallpaperKind,
};

// Media Foundation 原生支持的容器扩展名（见微软官方 Supported Media Formats in Media Foundation）。
// 这里声明的是「交给 Native 解码器尝试」，不保证容器内编码一定能解；编码不兼容时
// 渲染线程会回传 DecodeFailed，不回退 WebView。
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

    // M6 起视频不再走 WebView fallback：mkv / webm 等容器在当前阶段明确报错。
    // selection 参数保留给后续 M8 的动态 backend 切换，避免现在把接口收死。
    let _ = selection;
    Err(RendererError::new(
        RendererErrorCode::UnsupportedFormat,
        format!("unsupported video format: .{}", ext),
    ))
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
        assert_eq!(
            resolve_backend(
                WallpaperKind::Image,
                &path("webp"),
                BackendSelection::NativeOnly
            ),
            Ok(RenderBackend::Native)
        );
        assert_eq!(
            resolve_backend(
                WallpaperKind::Gif,
                &path("gif"),
                BackendSelection::NativeOnly
            ),
            Ok(RenderBackend::Native)
        );
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
    fn webm_and_mkv_are_rejected_without_webview_fallback() {
        for ext in ["webm", "mkv"] {
            for selection in [BackendSelection::NativeOnly, BackendSelection::Auto] {
                assert_eq!(
                    resolve_backend(WallpaperKind::Video, &path(ext), selection),
                    Err(RendererError::new(
                        RendererErrorCode::UnsupportedFormat,
                        format!("unsupported video format: .{}", ext)
                    )),
                    ".{} 当前阶段应明确报错而非回退 WebView",
                    ext
                );
            }
        }
    }

    #[test]
    fn unknown_video_extension_is_rejected() {
        assert_eq!(
            resolve_backend(WallpaperKind::Video, &path("flv"), BackendSelection::Auto),
            Err(RendererError::new(
                RendererErrorCode::UnsupportedFormat,
                "unsupported video format: .flv"
            ))
        );
    }
}
