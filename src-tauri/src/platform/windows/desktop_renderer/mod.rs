//! Windows 原生壁纸渲染器（DirectComposition + D3D11）。
//!
//! 架构：单一 renderer 线程跑消息循环与绘制，单一 video worker 拉帧，
//! 每个 VideoSession 独占自己的 GPU / SourceReader；renderer 默认做本地 NV12 copy，
//! fade 期间同时暂停视频时钟与音频。

// 独立音频线程：MF 拉 PCM 走 WASAPI 输出，与视频时钟低频校准。
mod audio;
// 图片一次性解码 worker：cache miss 时在独立线程解码 + 上传，避免冻结渲染线程。
mod image;
// 渲染线程：命令分发、帧推进、视频暂停 / 音频资格裁决与退出清理。
mod renderer;
// 渲染表面：壁纸三层状态、crossfade、纹理 / SRV / RTV 与 present。
mod surface;
// 视频播放：VideoPlayback / keyed mutex 共享纹理 / 单 worker 调度。
mod video;
// 壁纸原生窗口：创建、嵌入样式归一与销毁；消息泵实际在 renderer 线程。
mod window;

pub use renderer::DesktopRenderer;
pub use surface::{SharedDevice, SharedFrames};
