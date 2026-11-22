use std::env;
use std::path::Path;

use super::contracts::{
    BackendSelection, RenderBackend, RendererError, RendererErrorCode, WallpaperKind,
};

const NATIVE_VIDEO_EXTENSIONS: &[&str] = &["mp4", "m4v"];
const WEBVIEW_VIDEO_EXTENSIONS: &[&str] = &["webm", "mkv", "avi", "mov"];

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

    if matches!(selection, BackendSelection::NativeOnly) {
        return Err(RendererError::new(
            RendererErrorCode::UnsupportedFormat,
            format!(
                "native renderer does not support .{} video in NativeOnly mode",
                ext
            ),
        ));
    }

    if WEBVIEW_VIDEO_EXTENSIONS.contains(&ext.as_str()) {
        return Ok(RenderBackend::WebView);
    }

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
    fn mp4_uses_native_in_both_modes() {
        assert_eq!(
            resolve_backend(
                WallpaperKind::Video,
                &path("mp4"),
                BackendSelection::NativeOnly
            ),
            Ok(RenderBackend::Native)
        );
        assert_eq!(
            resolve_backend(WallpaperKind::Video, &path("m4v"), BackendSelection::Auto),
            Ok(RenderBackend::Native)
        );
    }

    #[test]
    fn fallback_video_only_exists_in_auto_mode() {
        assert_eq!(
            resolve_backend(WallpaperKind::Video, &path("webm"), BackendSelection::Auto),
            Ok(RenderBackend::WebView)
        );
        assert_eq!(
            resolve_backend(
                WallpaperKind::Video,
                &path("mkv"),
                BackendSelection::NativeOnly
            ),
            Err(RendererError::new(
                RendererErrorCode::UnsupportedFormat,
                "native renderer does not support .mkv video in NativeOnly mode"
            ))
        );
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
