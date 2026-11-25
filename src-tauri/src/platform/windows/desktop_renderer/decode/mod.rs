//! 解码 worker 层：帧的生产侧。
//!
//! 按格式分三组：image（纯函数 + 一次性 worker）、video / audio（常驻 worker）。
//! 产物统一是帧容器（`frames`）或播放状态 Arc，由渲染层消费。不依赖上层 render。

mod audio;
mod frames;
mod image;
mod video;

pub(crate) use audio::AudioPlayback;
pub(crate) use frames::{GpuFrame, SharedFrames, YuvCoeffs};
pub(crate) use image::{decode_and_upload, ImageDecodeQueue, Ready};
pub(crate) use video::{
    mf_shutdown, mf_startup, VideoFrameSnapshot, VideoIdentity, VideoPlayback, VideoWorkerHost,
    PAUSE_FADE, PAUSE_FULLSCREEN,
};
