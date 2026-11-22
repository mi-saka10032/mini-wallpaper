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

/// 壁纸素材类型，决定解码路径：
/// - `Image` / `Gif`：由 Native 侧 `image` crate 统一解码为 BGRA 帧序列；
/// - `Video`：走 Media Foundation 原生解码，是否在能力不足时回退 WebView 交给后端策略决定。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WallpaperKind {
    Image,
    Gif,
    Video,
}

impl WallpaperKind {
    /// 稳定的小写标识，用于日志与前端下发，避免各处重复硬编码字面量。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Gif => "gif",
            Self::Video => "video",
        }
    }
}

/// 真正承担渲染的后端。
/// `Native` 是 Windows 下直连 D3D11 的渲染线程；`WebView` 是「解码前」的兼容回退，
/// 仅用于能力表之外的视频。图片 / GIF 永远走 Native（见 `selection::resolve_backend`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RenderBackend {
    Native,
    WebView,
}

impl RenderBackend {
    /// 稳定标识；注意 WebView 对外用驼峰 `webView`，与前端枚举保持一致。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::WebView => "webView",
        }
    }
}

/// 后端选择策略，表达「用户/环境希望如何选后端」，与素材能力本身无关。
/// `Auto` 在 Native 能力不足时回退 WebView；`NativeOnly` 则直接报错、绝不回退，
/// 便于开发期尽早暴露兼容性问题（debug 构建默认走此策略）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackendSelection {
    NativeOnly,
    Auto,
}

impl BackendSelection {
    /// 稳定标识，供设置项持久化与前端展示。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NativeOnly => "nativeOnly",
            Self::Auto => "auto",
        }
    }
}

/// 多屏显示模式，取自系统/用户设置：
/// - `Independent`：各屏各贴各的壁纸；
/// - `Mirror` / `Extend`：一组屏共享同一张壁纸，靠裁剪视口区分每屏看到哪一块。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DisplayMode {
    Independent,
    Mirror,
    Extend,
}

impl DisplayMode {
    /// 解析前端下发的字符串；未知值返回 `None`，由调用方决定忽略还是报错。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "independent" => Some(Self::Independent),
            "mirror" => Some(Self::Mirror),
            "extend" => Some(Self::Extend),
            _ => None,
        }
    }

    /// 稳定标识，供持久化与前端展示。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Independent => "independent",
            Self::Mirror => "mirror",
            Self::Extend => "extend",
        }
    }
}

/// 图片如何映射到显示器矩形，语义对齐 CSS `object-fit`。
/// `Cover` / `Center` 的结果矩形可能超出目标区域，靠裁剪而非缩放对齐；
/// `Stretch` 在 CSS 中并不存在，前端写入时回退到 `Fill`，这里保持同等兼容（见 `fit::fit_rect`）。
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
    /// 解析前端下发的字符串；未知值返回 `None`，避免非法值进入几何计算。
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

    /// 稳定标识，供持久化与前端展示。
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

/// 显示器在虚拟桌面中的矩形，**单位为物理像素**，与 `CreateWindowExW` 坐标同源。
/// Windows 允许负坐标且主屏不一定在原点，所以 x / y 可能为负（见 `geometry::union_rect`）。
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

/// 一次渲染请求的完整快照，渲染线程据此决定「解码什么、贴到哪块屏、用哪个后端」。
/// 字段全部为拥有型数据而非引用/句柄，便于跨线程安全传递到渲染线程。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderDescriptor {
    /// 目标显示器 id，与 `ensure_monitor` 注册的 id 对应。
    pub monitor_id: String,
    /// 数据库主键，可为 `None`（例如仅预览、尚未落库的素材）。
    pub wallpaper_id: Option<i32>,
    /// 素材路径；同时作为帧缓存 key 的组成部分（见 `frame_cache::cache_key`）。
    pub file_path: PathBuf,
    /// 素材类型，决定解码路径。
    pub kind: WallpaperKind,
    /// 缩放/裁剪策略，决定几何映射。
    pub fit_mode: FitMode,
    /// 显示模式，决定是独立渲染还是参与跨屏裁剪。
    pub display_mode: DisplayMode,
    /// 由 `resolve_backend` 决策出的实际渲染后端。
    pub backend: RenderBackend,
    /// extend 模式下由调用方算好的本屏裁剪参数；其它模式为 `None`。
    pub extend_viewport: Option<ExtendViewport>,
}

/// 渲染错误分类，供前端区分「能力边界」与「运行时故障」：
/// `UnsupportedFormat` 是能力不支持，`DecodeFailed` / `DeviceLost` 是运行中失败，
/// `BackendSwitchFailed` 表示后端切换链路本身出错。
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
    /// 稳定的错误码字符串，作为上报给前端的 `code` 字段。
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

/// 带可读信息的渲染错误：`code` 供程序分支判断，`message` 供日志与前端展示。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RendererError {
    pub code: RendererErrorCode,
    pub message: String,
}

impl RendererError {
    /// 构造错误；`message` 接受任意可转为 `String` 的类型，便于直接传 `format!` 结果。
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
