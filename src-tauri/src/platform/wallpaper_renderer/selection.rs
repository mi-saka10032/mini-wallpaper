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

/// 决定默认的后端选择策略：debug / release 行为一致，统一用 `Auto` 按壁纸
/// capability 自动分流——不再在 debug 构建强制 `NativeOnly`，Auto fallback
/// 已完成实机验收（WebView 渲染、Native↔WebView 互切、资源释放均确认正常），
/// 不需要继续用 debug 强制 Native 来提前暴露兼容性问题。
/// 读取 `MINI_WALLPAPER_RENDERER=native` 仍可显式覆盖为 `NativeOnly`，
/// 用于单独验证原生链路——此时不支持的格式会直接报错，不回退 WebView。
pub fn default_backend_selection() -> BackendSelection {
    let forced = matches!(env::var("MINI_WALLPAPER_RENDERER").as_deref(), Ok("native"));
    if forced {
        BackendSelection::NativeOnly
    } else {
        BackendSelection::Auto
    }
}

/// 根据素材类型、文件扩展名与选择策略决定实际渲染后端。
/// 图片 / GIF 恒为 Native；视频仅在能力表（扩展名）内才走 Native，表外格式
/// 在 `Auto` 下回退 WebView、`NativeOnly` 下明确报错。
/// 这是「解码前」的 capability 决策：一旦选定 Native，解码失败只回传错误，不再二次回退。
pub fn resolve_backend(
    kind: WallpaperKind,
    file_path: &Path,
    selection: BackendSelection,
) -> Result<RenderBackend, RendererError> {
    // 图片 / GIF 的解码能力无需回退，Native 统一处理（与扩展名无关）。
    if matches!(kind, WallpaperKind::Image | WallpaperKind::Gif) {
        return Ok(RenderBackend::Native);
    }

    let ext = file_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    // 容器扩展名在 Native 能力表内，交给 Media Foundation 解码；
    // 注意这里只判断容器，编码不兼容会在真正的解码阶段失败（见上方常量注释）。
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
