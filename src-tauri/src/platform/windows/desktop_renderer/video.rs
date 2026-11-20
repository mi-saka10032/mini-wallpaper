//! Media Foundation 流式视频解码（Windows 专用，只在 renderer 线程使用）
//!
//! 为什么不做成「解码全部帧进 SharedFrames」：一段 30fps / 4K / 10s 视频的原始
//! BGRA 约 10GB，全帧常驻必然爆显存。视频走流式：SourceReader 按 100ns 时间戳
//! 逐帧取出，上传进**一张**可复用的 D3D11 纹理，所有 surface 绑定同一个 SRV。
//!
//! 同组多屏为什么能同帧：`VideoPlayback` 由 renderer loop 每帧只推进一次，
//! 组内 surface 共享同一个 `Arc<VideoPlayback>`，绘制时读到的 SRV 必然是同一帧。
//! 这与 GIF 的 `SharedFrames` 是两套机制，但共享同一个 device / pipeline。
//!
//! 静音：SourceReader 只选视频流、关掉音频流，不做音频渲染；音量由后续里程碑补。
//! 暂停：冻结播放时钟，不 flush / 不销毁 SourceReader，恢复后从原时间点继续。

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use windows::core::GUID;
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFMediaBuffer, IMFMediaType, IMFSample, IMFSourceReader, MFCreateAttributes,
    MFCreateMediaType, MFCreateSourceReaderFromURL, MFMediaType_Video, MFStartup,
    MFVideoFormat_RGB32, MFSTARTUP_FULL, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE,
    MF_MT_SUBTYPE, MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READER_ALL_STREAMS,
    MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_VERSION,
};

/// MF 平台只需启动一次；放在 renderer 线程内由 ComScope 保证 COM 已初始化。
/// 这里不主动 MFShutdown：进程退出时系统回收，避免与仍在运行的 SourceReader 竞争。
pub fn startup() -> Result<()> {
    unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.context("Media Foundation 初始化失败")
}

/// 当前视频帧：一张复用的纹理 + SRV，尺寸随媒体类型变化而重建。
struct FrameTexture {
    view: windows::Win32::Graphics::Direct3D11::ID3D11ShaderResourceView,
    texture: windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    width: u32,
    height: u32,
}

/// 一组显示器共享的视频播放状态。
///
/// 所有字段只在 renderer 线程访问（source reader / D3D11 即时上下文都不是线程安全的），
/// 跨屏靠 `Arc` 共享同一个实例，不靠锁。
pub struct VideoPlayback {
    reader: IMFSourceReader,
    device: windows::Win32::Graphics::Direct3D11::ID3D11Device,
    context: windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    texture: Option<FrameTexture>,
    /// 播放起点；`now - started_at - paused_total` 即当前视频时间
    started_at: Instant,
    paused_total: Duration,
    paused_at: Option<Instant>,
    /// 当前帧的视频时间（100ns → Duration）
    frame_time: Duration,
    /// 当前帧持续时长，决定何时取下一帧
    frame_duration: Duration,
    /// 循环时SourceReader回到开头，用 cycle_base 保持 frame_time 单调递增
    cycle_base: Duration,
    /// 播放列表总时长（最后一帧时间 + 时长），用于循环；未知时为 0
    loop_duration: Duration,
}

impl VideoPlayback {
    /// 打开视频并请求 RGB32 输出。
    ///
    /// RGB32 在 little-endian 上的字节序就是 BGRA，正好匹配 DXGI_FORMAT_B8G8R8A8_UNORM，
    /// shader 再强制 alpha=1，因此无需 CPU 侧 RGBA→BGRA（高分辨率视频这步很贵）。
    pub fn open(
        device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
        context: &windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
        path: &Path,
    ) -> Result<Self> {
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let url = windows::core::PCWSTR(wide.as_ptr());

        let attributes: IMFAttributes = unsafe {
            let mut value = None;
            MFCreateAttributes(&mut value, 0)?;
            value.context("MF 属性对象创建失败")?
        };
        let reader = unsafe { MFCreateSourceReaderFromURL(url, &attributes) }
            .with_context(|| format!("Media Foundation 无法打开视频 {:?}", path))?;

        // 关掉所有流再只开第一个视频流：视频壁纸不播音频。
        unsafe {
            reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
            reader.SetStreamSelection(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, true)?;
        }

        let output_type: IMFMediaType = unsafe { MFCreateMediaType() }?;
        unsafe {
            output_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            output_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
            reader.SetCurrentMediaType(
                MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                None,
                &output_type,
            )?;
        }

        let negotiated =
            unsafe { reader.GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)? };
        let subtype = unsafe { negotiated.GetGUID(&MF_MT_SUBTYPE)? };
        if subtype != MFVideoFormat_RGB32 {
            return Err(anyhow!(
                "视频解码器不支持 RGB32 输出（实际 subtype={:?}），M6 不做 NV12/CPU 转换",
                subtype
            ));
        }

        let mut playback = Self {
            reader,
            device: device.clone(),
            context: context.clone(),
            texture: None,
            started_at: Instant::now(),
            paused_total: Duration::ZERO,
            paused_at: None,
            frame_time: Duration::ZERO,
            frame_duration: Duration::from_millis(40),
            cycle_base: Duration::ZERO,
            loop_duration: Duration::ZERO,
        };

        // 先取一帧，确保打开阶段就能发现解码失败（错误需要在 set_wallpaper 时回传）。
        playback.read_next_frame()?;
        Ok(playback)
    }

    /// 每帧调用一次：根据墙钟推进到当前应显示的帧。
    ///
    /// 暂停期间调用方不再渲染，这里也不推进；恢复时把暂停时长计入 `paused_total`，
    /// 视频时间因此冻结在原处，SourceReader 不 flush、不重建。
    pub fn advance(&mut self, now: Instant) -> Result<()> {
        if self.paused_at.is_some() {
            return Ok(());
        }
        let target = now
            .saturating_duration_since(self.started_at)
            .saturating_sub(self.paused_total);

        // 一帧渲染周期内最多补 8 帧，防止后台恢复后疯狂追帧阻塞渲染线程。
        for _ in 0..8 {
            if target < self.frame_time.saturating_add(self.frame_duration) {
                return Ok(());
            }
            if self.read_next_frame()? {
                continue;
            }
            // EOF：从头循环，保留累计时长让时间戳单调递增。
            self.cycle_base = self.cycle_base.saturating_add(self.loop_duration);
            unsafe {
                self.reader
                    .Flush(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)?;
                let position = seek_zero_position();
                self.reader
                    .SetCurrentPosition(&GUID::default(), &position)?;
            }
            if !self.read_next_frame()? {
                return Err(anyhow!("视频循环后仍无法读取首帧"));
            }
        }
        Ok(())
    }

    /// crossfade 结束时调用：视频时间从 `now` 重新由 0 开始。
    ///
    /// fade 期间调用方不 advance，SourceReader 停在 open 时预读的首帧之后；
    /// 这里 flush + seek(0) 并重新预读，丢掉的只是那张预读帧，不会打断正在播放的内容。
    /// 不销毁 SourceReader，和暂停恢复保持同一套生命周期。
    pub fn reset_clock(&mut self, now: Instant) -> Result<()> {
        self.started_at = now;
        self.paused_total = Duration::ZERO;
        self.paused_at = None;
        self.frame_time = Duration::ZERO;
        self.frame_duration = Duration::from_millis(40);
        self.cycle_base = Duration::ZERO;
        self.loop_duration = Duration::ZERO;
        unsafe {
            self.reader
                .Flush(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)?;
            let position = seek_zero_position();
            self.reader
                .SetCurrentPosition(&GUID::default(), &position)?;
        }
        if !self.read_next_frame()? {
            return Err(anyhow!("视频重置时钟后无法读取首帧"));
        }
        Ok(())
    }

    pub fn set_paused(&mut self, paused: bool, now: Instant) {
        match (paused, self.paused_at) {
            (true, None) => self.paused_at = Some(now),
            (false, Some(at)) => {
                self.paused_total = self
                    .paused_total
                    .saturating_add(now.saturating_duration_since(at));
                self.paused_at = None;
            }
            _ => {}
        }
    }

    /// 当前帧的 SRV 与尺寸（SRV 是 COM 引用计数，clone 很便宜）。
    /// 返回 owned 值，避免跨 MutexGuard 暴露借用。
    pub fn frame_snapshot(
        &self,
    ) -> Option<(
        windows::Win32::Graphics::Direct3D11::ID3D11ShaderResourceView,
        u32,
        u32,
    )> {
        self.texture
            .as_ref()
            .map(|t| (t.view.clone(), t.width, t.height))
    }

    /// 读取下一帧并上传到复用纹理；返回 false 表示 EOF。
    fn read_next_frame(&mut self) -> Result<bool> {
        let mut flags = 0u32;
        let mut timestamp = 0i64;
        let mut sample: Option<IMFSample> = None;
        unsafe {
            self.reader.ReadSample(
                MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                0,
                None,
                Some(&mut flags),
                Some(&mut timestamp),
                Some(&mut sample),
            )?;
        }

        if (flags as i32) & MF_SOURCE_READERF_ENDOFSTREAM.0 != 0 || sample.is_none() {
            return Ok(false);
        }
        let sample = sample.context("SourceReader 返回空 sample")?;

        let duration = unsafe { sample.GetSampleDuration() }.unwrap_or(0);
        let frame_duration = hns_to_duration(duration.max(0) as u64);
        let frame_time = self
            .cycle_base
            .saturating_add(hns_to_duration(timestamp.max(0) as u64));

        // 只在首轮估算循环时长；后续轮次 cycle_base 已承接，不需要重算。
        if self.cycle_base.is_zero() {
            self.loop_duration = frame_time.saturating_add(frame_duration);
        }

        let buffer = unsafe { sample.ConvertToContiguousBuffer() }?;
        self.upload_buffer(&buffer)?;

        self.frame_time = frame_time;
        self.frame_duration = if frame_duration.is_zero() {
            Duration::from_millis(40)
        } else {
            frame_duration
        };
        Ok(true)
    }

    /// 把 RGB32 缓冲区上传到一张复用的 D3D11 纹理。
    fn upload_buffer(&mut self, buffer: &IMFMediaBuffer) -> Result<()> {
        use windows::Win32::Graphics::Direct3D11::{
            ID3D11ShaderResourceView, ID3D11Texture2D, D3D11_BIND_SHADER_RESOURCE,
            D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
        };
        use windows::Win32::Graphics::Dxgi::Common::{
            DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
        };

        let mut ptr: *mut u8 = ptr::null_mut();
        let mut max_len = 0u32;
        let mut current_len = 0u32;
        unsafe { buffer.Lock(&mut ptr, Some(&mut max_len), Some(&mut current_len))? };
        let result = (|| -> Result<()> {
            if ptr.is_null() {
                return Err(anyhow!("Video sample 缓冲区指针为空"));
            }

            // 上界用 max_len：MF 的 cbCurrentLength 可能只算有效像素（不含 stride padding），
            // 若按它逐行校验会把带 padding 的最后一行误判越界。
            let buffer_len = max_len.max(current_len) as usize;
            // stride 可能是负数（自底向上）或带对齐；统一按 |stride| 读取每行。
            let raw_stride = unsafe {
                self.reader
                    .GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)?
                    .GetUINT64(&MF_MT_DEFAULT_STRIDE)?
            } as i64;
            let stride = if raw_stride == 0 {
                // 无 stride 时按 width 推，但此处还没读 frame_size，先用当前像素数上界；
                // 后续拿到 width 后会用 expected 覆盖。
                buffer_len.max(1)
            } else {
                raw_stride.unsigned_abs() as usize
            };
            let bottom_up = raw_stride < 0;

            // 帧尺寸以媒体类型为准，不能沿用上一帧（分辨率可能中途变化）。
            let media_type = unsafe {
                self.reader
                    .GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)?
            };
            let frame_size = unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE)? };
            let width = (frame_size >> 32) as u32;
            let height = frame_size as u32;
            if width == 0 || height == 0 {
                return Err(anyhow!("视频帧尺寸无效: {}x{}", width, height));
            }
            let expected = width as usize * 4;
            // MF 通常给出 stride；缺失时按无 padding 的行字节数处理。
            let stride = if raw_stride == 0 { expected } else { stride };
            if stride < expected {
                return Err(anyhow!(
                    "视频 stride({}) 小于行字节数({})，拒绝上传避免越界读取",
                    stride,
                    expected
                ));
            }

            let need_rebuild = self
                .texture
                .as_ref()
                .map(|t| t.width != width || t.height != height)
                .unwrap_or(true);
            if need_rebuild {
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: width,
                    Height: height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                };
                let mut texture: Option<ID3D11Texture2D> = None;
                unsafe {
                    self.device
                        .CreateTexture2D(&desc, None, Some(&mut texture))?
                };
                let texture = texture.context("视频帧纹理创建失败")?;
                let mut view: Option<ID3D11ShaderResourceView> = None;
                unsafe {
                    self.device
                        .CreateShaderResourceView(&texture, None, Some(&mut view))?
                };
                let view = view.context("视频帧 SRV 创建失败")?;
                self.texture = Some(FrameTexture {
                    view,
                    texture,
                    width,
                    height,
                });
            }

            // 只上传有效像素区；stride 的 padding 不进 GPU。
            let texture = &self.texture.as_ref().expect("纹理已就绪").texture;
            for row in 0..height as usize {
                let src_row = if bottom_up {
                    height as usize - 1 - row
                } else {
                    row
                };
                let start = src_row * stride;
                let end = start + expected;
                if end > buffer_len {
                    return Err(anyhow!(
                        "视频 sample 数据不足: 行 {} 需要 {}..{}，实际 {}",
                        row,
                        start,
                        end,
                        buffer_len
                    ));
                }
                unsafe {
                    self.context.UpdateSubresource(
                        texture,
                        0,
                        None,
                        ptr.add(start) as *const c_void,
                        expected as u32,
                        0,
                    );
                }
            }
            Ok(())
        })();
        unsafe { buffer.Unlock()? };
        result
    }
}

/// hns（100ns）转 Duration；溢出时钳到最大值，避免 panic。
fn hns_to_duration(hns: u64) -> Duration {
    let nanos = (hns as u128).saturating_mul(100);
    Duration::from_nanos(nanos.min(u128::from(u64::MAX)) as u64)
}

/// Seek 到 0 的 PROPVARIANT：VT_I8 + hVal=0。
///
/// MF_SOURCE_READER 的 SetCurrentPosition 用 GUID_NULL 表示「按 100ns 绝对时间」，
/// 这里只回退到开头，不涉及复杂时间格式。
fn seek_zero_position() -> windows::Win32::System::Com::StructuredStorage::PROPVARIANT {
    use windows::Win32::System::Com::StructuredStorage::{
        PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
    };
    use windows::Win32::System::Variant::VT_I8;

    PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_I8,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: PROPVARIANT_0_0_0 { hVal: 0 },
            }),
        },
    }
}
