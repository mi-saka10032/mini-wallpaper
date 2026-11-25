//! 已上传 GPU 的帧数据与帧时钟。
//!
//! `SharedFrames` 是解码侧（image worker）与渲染侧（RenderSurface）的交汇点：
//! 多屏共享同一个 Arc 才能选到同一帧，避免跨屏接缝出现不同画面。

use std::time::{Duration, Instant};

use windows::Win32::Graphics::Direct3D11::ID3D11ShaderResourceView;

/// NV12 → RGB 的转换系数，由视频流的颜色矩阵 / range 推导。
///
/// 放在 decode 侧算：矩阵与 range 是视频文件的元数据，由 SourceReader 读出；
/// render 侧只接收这组 float 写入 cbuffer，不依赖 Media Foundation 常量。
/// 系数推导：BT.601/709 的 Kr/Kb 权重 → full-range 系数，乘 limited-range
/// 缩放（Y 用 255/219、UV 用 255/224）；full range 缩放为 1。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct YuvCoeffs {
    /// Y 的零点偏移：limited = 16/255，full = 0。
    pub y_offset: f32,
    /// Y 的幅度缩放：limited = 255/219，full = 1。
    pub y_scale: f32,
    /// R ← V（Cr）系数。
    pub r_v: f32,
    /// G ← U（Cb）系数（负值）。
    pub g_u: f32,
    /// G ← V（Cr）系数（负值）。
    pub g_v: f32,
    /// B ← U（Cb）系数。
    pub b_u: f32,
}

impl YuvCoeffs {
    /// 按 BT.601/709 选择权重，并按 limited / full range 缩放。
    pub fn new(bt709: bool, limited: bool) -> Self {
        let (kr, kb) = if bt709 {
            (0.2126f32, 0.0722f32)
        } else {
            (0.299f32, 0.114f32)
        };
        let kg = 1.0 - kr - kb;
        let (y_offset, y_scale, uv_scale) = if limited {
            (16.0 / 255.0, 255.0 / 219.0, 255.0 / 224.0)
        } else {
            (0.0, 1.0, 1.0)
        };
        Self {
            y_offset,
            y_scale,
            r_v: uv_scale * 2.0 * (1.0 - kr),
            g_u: uv_scale * 2.0 * kb * (1.0 - kb) / kg,
            g_v: uv_scale * 2.0 * kr * (1.0 - kr) / kg,
            b_u: uv_scale * 2.0 * (1.0 - kb),
        }
    }
}

/// 默认系数：历史行为 BT.601 + limited，元数据缺失时的回退值。
impl Default for YuvCoeffs {
    fn default() -> Self {
        Self::new(false, true)
    }
}

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
