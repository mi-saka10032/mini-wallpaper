//! 已上传 GPU 的帧数据与帧时钟。
//!
//! `SharedFrames` 是解码侧（image worker）与渲染侧（RenderSurface）的交汇点：
//! 多屏共享同一个 Arc 才能选到同一帧，避免跨屏接缝出现不同画面。

use std::time::{Duration, Instant};

use windows::Win32::Graphics::Direct3D11::ID3D11ShaderResourceView;

/// 一张已上传 GPU 的帧：`view` 是 SRV（内部持有纹理引用），duration 供动图取模选帧。
pub(crate) struct GpuFrame {
    pub(crate) view: ID3D11ShaderResourceView,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) duration: Duration,
}

pub struct SharedFrames {
    frames: Vec<GpuFrame>,
    // 组内所有 surface 共用这个起点，保证跨屏选到同一帧。
    started_at: Instant,
}

impl SharedFrames {
    /// 由 image worker 组装帧序列；帧时钟起点在 attach 到 surface 时才由
    /// renderer 线程通过 `reset_clock` 对齐，这里给一个占位起点。
    pub(crate) fn new(frames: Vec<GpuFrame>) -> Self {
        Self {
            frames,
            started_at: Instant::now(),
        }
    }

    /// 把帧时钟起点对齐到真正 attach 的时刻，保证 GIF 从第一帧开始、
    /// 组内所有屏共享同一条时间线。
    pub(crate) fn reset_clock(&mut self, started_at: Instant) {
        self.started_at = started_at;
    }

    /// 多帧动图需要按帧时钟持续重绘；单帧静态图不需要。
    pub(crate) fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }

    pub(crate) fn current_frame(&self, now: Instant) -> &GpuFrame {
        if self.frames.len() <= 1 {
            return &self.frames[0];
        }

        // 按总时长取模，让 GIF / 动态 WebP 循环播放而不是播完一轮停在首帧
        let total: Duration = self.frames.iter().map(|frame| frame.duration).sum();
        if total.is_zero() {
            return &self.frames[0];
        }
        let elapsed = now.saturating_duration_since(self.started_at).as_nanos() % total.as_nanos();

        let mut accumulated = 0u128;
        for frame in &self.frames {
            accumulated += frame.duration.as_nanos();
            if elapsed < accumulated {
                return frame;
            }
        }

        &self.frames[0]
    }
}
