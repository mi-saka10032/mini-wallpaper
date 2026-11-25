//! 渲染层：帧的消费侧与呈现。
//!
//! `SharedDevice` 是本层的资源聚合根，持有 device / context / pipeline / video worker；
//! 每个显示器一个 `RenderSurface`，通过 `draw_media` 把帧绘制到 swapchain 并 Present。
//! 依赖下层 device（窗口 / COM / 错误判定）与 decode（帧容器 / 播放状态）。

mod composition;
mod pipeline;
mod render_surface;
mod shared_device;

pub(crate) use render_surface::{RenderSurface, WallpaperMedia};
pub(crate) use shared_device::SharedDevice;
// pipeline / composition 的私有类型只在本层内使用，不经层门面暴露。
