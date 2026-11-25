//! 壁纸渲染表面与共享 GPU 资源。
//!
//! 按设计边界拆成 6 个子模块；本文件只做声明与 re-export，
//! 使外部引用路径 `super::surface::{...}` 保持不变（纯内部重排）。

// 单屏渲染状态与绘制（WallpaperMedia / Transition / RenderSurface）
mod render_surface;
// 已上传 GPU 的帧数据与帧时钟
mod frames;
// 全屏 quad 的 HLSL 与 D3D11 管线资源
mod pipeline;
// 共享 D3D11 context 与 video worker 宿主
mod device;
// DirectComposition swapchain 创建
mod composition;
// D3D11 / DXGI 设备丢失错误判定
mod device_lost;

pub(crate) use device::SharedDevice;
pub(crate) use frames::{GpuFrame, SharedFrames};
pub(crate) use render_surface::{RenderSurface, WallpaperMedia};
// QuadPipeline / FadeConstants / Vertex 只在本模块内跨文件使用，不需对外 re-export。
