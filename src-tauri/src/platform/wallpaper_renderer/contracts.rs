use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Native↔Native 换图淡入淡出的固定时长。不暴露设置项：壁纸切换属于低频操作，
/// 固定值既让跨屏共用同一条时间线，也省掉设置项到 renderer 线程的同步链路。
pub const CROSSFADE_DURATION: Duration = Duration::from_millis(350);

/// 把已流逝时间归一化为 [0, 1] 的 fade 进度；零时长视作立即完成。
pub fn crossfade_progress(elapsed: Duration, duration: Duration) -> f32 {
    if duration.is_zero() {
        return 1.0;
    }
    (elapsed.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WallpaperKind {
    Image,
    Gif,
    Video,
}

impl WallpaperKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Gif => "gif",
            Self::Video => "video",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RenderBackend {
    Native,
    WebView,
}

impl RenderBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::WebView => "webView",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackendSelection {
    NativeOnly,
    Auto,
}

impl BackendSelection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NativeOnly => "nativeOnly",
            Self::Auto => "auto",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DisplayMode {
    Independent,
    Mirror,
    Extend,
}

impl DisplayMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "independent" => Some(Self::Independent),
            "mirror" => Some(Self::Mirror),
            "extend" => Some(Self::Extend),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Independent => "independent",
            Self::Mirror => "mirror",
            Self::Extend => "extend",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FitMode {
    Cover,
    Contain,
    Fill,
    Stretch,
    Center,
}

impl FitMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "cover" => Some(Self::Cover),
            "contain" => Some(Self::Contain),
            "fill" => Some(Self::Fill),
            "stretch" => Some(Self::Stretch),
            "center" => Some(Self::Center),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cover => "cover",
            Self::Contain => "contain",
            Self::Fill => "fill",
            Self::Stretch => "stretch",
            Self::Center => "center",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// extend 模式下单个显示器的裁剪参数。
///
/// 单位统一为**物理像素的虚拟桌面坐标**，与 `MonitorRect` / `CreateWindowExW` 一致；
/// `offset_x/y` 相对所有显示器并集的左上角，不是相对主屏，因此负坐标布局同样成立。
/// 渲染时令 viewport 为 `(0, 0, total_width, total_height) − (offset_x, offset_y)`，
/// 等价于 WebView 侧 `object-fit: fill` 的全图容器叠加 `left/top: -offset`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtendViewport {
    pub offset_x: i32,
    pub offset_y: i32,
    pub total_width: i32,
    pub total_height: i32,
    /// 本屏尺寸，用于裁剪结果自检；前端 `useExtendViewport` 的 myWidth/myHeight
    pub monitor_width: i32,
    pub monitor_height: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderDescriptor {
    pub monitor_id: String,
    pub wallpaper_id: Option<i32>,
    pub file_path: PathBuf,
    pub kind: WallpaperKind,
    pub fit_mode: FitMode,
    pub display_mode: DisplayMode,
    pub backend: RenderBackend,
    pub extend_viewport: Option<ExtendViewport>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RendererErrorCode {
    UnsupportedFormat,
    BackendUnavailable,
    DecodeFailed,
    DeviceLost,
    EmbedFailed,
    BackendSwitchFailed,
}

impl RendererErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedFormat => "UnsupportedFormat",
            Self::BackendUnavailable => "BackendUnavailable",
            Self::DecodeFailed => "DecodeFailed",
            Self::DeviceLost => "DeviceLost",
            Self::EmbedFailed => "EmbedFailed",
            Self::BackendSwitchFailed => "BackendSwitchFailed",
        }
    }
}

impl fmt::Display for RendererErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RendererError {
    pub code: RendererErrorCode,
    pub message: String,
}

impl RendererError {
    pub fn new(code: RendererErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for RendererError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RendererError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_mode_parse_accepts_known_values() {
        assert_eq!(
            DisplayMode::parse("independent"),
            Some(DisplayMode::Independent)
        );
        assert_eq!(DisplayMode::parse("mirror"), Some(DisplayMode::Mirror));
        assert_eq!(DisplayMode::parse("extend"), Some(DisplayMode::Extend));
        assert_eq!(DisplayMode::parse("unknown"), None);
    }

    #[test]
    fn fit_mode_parse_accepts_known_values() {
        assert_eq!(FitMode::parse("cover"), Some(FitMode::Cover));
        assert_eq!(FitMode::parse("center"), Some(FitMode::Center));
        assert_eq!(FitMode::parse("unknown"), None);
    }

    #[test]
    fn crossfade_progress_is_clamped_and_handles_zero_duration() {
        assert_eq!(crossfade_progress(Duration::ZERO, CROSSFADE_DURATION), 0.0);
        assert_eq!(
            crossfade_progress(Duration::from_millis(175), CROSSFADE_DURATION),
            0.5
        );
        assert_eq!(
            crossfade_progress(Duration::from_millis(1000), CROSSFADE_DURATION),
            1.0
        );
        assert_eq!(
            crossfade_progress(Duration::from_secs(1), Duration::ZERO),
            1.0
        );
    }
}
