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
//! 音频：SourceReader 只选视频流；旁挂的 AudioPlayback 只负责把同一文件的音频
//! 送到系统输出，视频帧与时钟仍由本模块独占控制。无音轨时静音降级。
//! 暂停：冻结播放时钟，不 flush / 不销毁 SourceReader，恢复后从原时间点继续。

use std::mem::ManuallyDrop;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{self, NonNull};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use windows::core::{Interface, BOOL, GUID};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView, ID3D11Texture2D,
    ID3D11VideoContext, ID3D11VideoDevice, ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator,
    ID3D11VideoProcessorInputView, ID3D11VideoProcessorOutputView, D3D11_BIND_RENDER_TARGET,
    D3D11_BIND_SHADER_RESOURCE, D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV, D3D11_USAGE_DEFAULT,
    D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
    D3D11_VPIV_DIMENSION_TEXTURE2D, D3D11_VPOV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};

use super::audio::AudioPlayback;
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFDXGIBuffer, IMFDXGIDeviceManager, IMFMediaBuffer, IMFMediaType, IMFSample,
    IMFSourceReader, MFCreateAttributes, MFCreateDXGIDeviceManager, MFCreateMediaType,
    MFCreateSourceReaderFromURL, MFMediaType_Video, MFStartup, MFVideoFormat_NV12,
    MFVideoFormat_RGB32, MFSTARTUP_FULL, MF_E_NO_MORE_TYPES, MF_MT_DEFAULT_STRIDE,
    MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS,
    MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READER_ALL_STREAMS, MF_SOURCE_READER_D3D_MANAGER,
    MF_SOURCE_READER_DISABLE_DXVA, MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_VERSION,
};

/// MF 平台只需启动一次；放在 renderer 线程内由 ComScope 保证 COM 已初始化。
/// 这里不主动 MFShutdown：进程退出时系统回收，避免与仍在运行的 SourceReader 竞争。
pub fn startup() -> Result<()> {
    unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.context("Media Foundation 初始化失败")
}

/// 当前视频帧的 GPU 资源。RGB32 只有主纹理；NV12 拆成 Y / UV 两张平面纹理。
struct FrameTexture {
    view: ID3D11ShaderResourceView,
    texture: ID3D11Texture2D,
    uv_view: Option<ID3D11ShaderResourceView>,
    uv_texture: Option<ID3D11Texture2D>,
    pixel_format: VideoPixelFormat,
    width: u32,
    height: u32,
}

/// renderer 绘制一帧视频所需的全部 SRV；NV12 时 uv_view 为 Some。
pub struct VideoFrameSnapshot {
    pub view: ID3D11ShaderResourceView,
    pub uv_view: Option<ID3D11ShaderResourceView>,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VideoPixelFormat {
    Bgra8,
    Nv12,
}

/// DXVA 解码纹理不能直接当普通 SRV 用，统一经 VideoProcessor 转到 BGRA8。
struct VideoProcessorState {
    device: ID3D11Device,
    video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,
    enumerator: Option<ID3D11VideoProcessorEnumerator>,
    processor: Option<ID3D11VideoProcessor>,
    output_texture: Option<ID3D11Texture2D>,
    output_view: Option<ID3D11VideoProcessorOutputView>,
    width: u32,
    height: u32,
}

/// 视频链路计时样本：每 2s 聚合输出一次，避免每帧刷日志。
#[derive(Default)]
struct VideoTimingStats {
    frames: u64,
    read_total: Duration,
    read_max: Duration,
    upload_total: Duration,
    upload_max: Duration,
    resets: u64,
    reset_total: Duration,
    reset_max: Duration,
    last_report: Option<Instant>,
}

impl VideoTimingStats {
    fn record_frame(&mut self, read: Duration, upload: Duration) {
        self.frames = self.frames.saturating_add(1);
        self.read_total = self.read_total.saturating_add(read);
        self.upload_total = self.upload_total.saturating_add(upload);
        self.read_max = self.read_max.max(read);
        self.upload_max = self.upload_max.max(upload);
    }

    fn record_reset(&mut self, reset: Duration) {
        self.resets = self.resets.saturating_add(1);
        self.reset_total = self.reset_total.saturating_add(reset);
        self.reset_max = self.reset_max.max(reset);
    }

    fn report_if_due(&mut self, label: &str) {
        let now = Instant::now();
        let due = self
            .last_report
            .map(|at| now.saturating_duration_since(at) >= Duration::from_secs(2))
            .unwrap_or(true);
        if !due || self.frames == 0 {
            return;
        }

        let frames = self.frames;
        log::info!(
            "[Video] 播放计时 file={} frames={} read_avg_ms={:.2} read_max_ms={:.2} upload_avg_ms={:.2} upload_max_ms={:.2} resets={} reset_avg_ms={:.2} reset_max_ms={:.2}",
            label,
            frames,
            ms_per(self.read_total, frames),
            self.read_max.as_secs_f64() * 1000.0,
            ms_per(self.upload_total, frames),
            self.upload_max.as_secs_f64() * 1000.0,
            self.resets,
            ms_per(self.reset_total, self.resets.max(1)),
            self.reset_max.as_secs_f64() * 1000.0
        );

        *self = Self {
            last_report: Some(now),
            ..Self::default()
        };
    }
}

fn ms_per(total: Duration, count: u64) -> f64 {
    total.as_secs_f64() * 1000.0 / count.max(1) as f64
}

/// 按微软推荐配置 SourceReader 的 DXVA 属性。
fn create_dxgi_device_manager(
    attributes: &IMFAttributes,
    device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
    label: &str,
) -> Option<IMFDXGIDeviceManager> {
    let mut reset_token = 0u32;
    let mut manager: Option<IMFDXGIDeviceManager> = None;
    let result = (|| -> windows::core::Result<()> {
        unsafe { MFCreateDXGIDeviceManager(&mut reset_token, &mut manager)? };
        let manager = manager
            .as_ref()
            .ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))?;
        unsafe {
            manager.ResetDevice(device, reset_token)?;
            attributes.SetUnknown(&MF_SOURCE_READER_D3D_MANAGER, manager)?;
            attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
            attributes.SetUINT32(&MF_SOURCE_READER_DISABLE_DXVA, 0)?;
            // 不设置 MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING：微软文档明确说它太慢。
        }
        Ok(())
    })();

    match result {
        Ok(()) => {
            log::info!("[Video] DXVA SourceReader 已配置 file={}", label);
            manager
        }
        Err(err) => {
            log::warn!(
                "[Video] DXVA SourceReader 配置失败，回退软件解码 file={} err={:#}",
                label,
                err
            );
            None
        }
    }
}

impl VideoProcessorState {
    fn new(device: &ID3D11Device, context: &ID3D11DeviceContext) -> Result<Self> {
        Ok(Self {
            device: device.clone(),
            video_device: device.cast()?,
            video_context: context.cast()?,
            enumerator: None,
            processor: None,
            output_texture: None,
            output_view: None,
            width: 0,
            height: 0,
        })
    }

    fn ensure_resources(&mut self, width: u32, height: u32) -> Result<()> {
        use windows::Win32::Graphics::Direct3D11::D3D11_TEXTURE2D_DESC;

        if self.width == width
            && self.height == height
            && self.processor.is_some()
            && self.output_view.is_some()
        {
            return Ok(());
        }

        let rate = DXGI_RATIONAL {
            Numerator: 30,
            Denominator: 1,
        };
        let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: rate,
            InputWidth: width,
            InputHeight: height,
            OutputFrameRate: rate,
            OutputWidth: width,
            OutputHeight: height,
            Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
        };
        let enumerator = unsafe { self.video_device.CreateVideoProcessorEnumerator(&content) }
            .context("创建 VideoProcessor enumerator 失败")?;
        let processor = unsafe { self.video_device.CreateVideoProcessor(&enumerator, 0) }
            .context("创建 VideoProcessor 失败")?;

        let output = D3D11_TEXTURE2D_DESC {
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
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut texture: Option<ID3D11Texture2D> = None;
        unsafe {
            self.device
                .CreateTexture2D(&output, None, Some(&mut texture))?;
        }
        let texture = texture.context("DXVA 输出纹理创建失败")?;

        let view_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
            },
        };
        let mut output_view: Option<ID3D11VideoProcessorOutputView> = None;
        unsafe {
            self.video_device.CreateVideoProcessorOutputView(
                &texture,
                &enumerator,
                &view_desc,
                Some(&mut output_view),
            )?
        };
        let output_view = output_view.context("创建 VideoProcessor output view 失败")?;

        self.enumerator = Some(enumerator);
        self.processor = Some(processor);
        self.output_texture = Some(texture);
        self.output_view = Some(output_view);
        self.width = width;
        self.height = height;
        Ok(())
    }

    fn process(
        &mut self,
        dxgi_buffer: &IMFDXGIBuffer,
        width: u32,
        height: u32,
    ) -> Result<ID3D11ShaderResourceView> {
        self.ensure_resources(width, height)?;
        let enumerator = self
            .enumerator
            .as_ref()
            .context("VideoProcessor enumerator 缺失")?;
        let processor = self.processor.as_ref().context("VideoProcessor 缺失")?;
        let output_view = self
            .output_view
            .as_ref()
            .context("VideoProcessor output view 缺失")?;

        let mut raw: *mut std::ffi::c_void = ptr::null_mut();
        unsafe { dxgi_buffer.GetResource(&ID3D11Texture2D::IID, &mut raw)? };
        let raw = NonNull::new(raw).context("DXVA sample 返回空 texture")?;
        let decoder_texture: ID3D11Texture2D = unsafe { ID3D11Texture2D::from_raw(raw.as_ptr()) };
        let subresource = unsafe { dxgi_buffer.GetSubresourceIndex()? };

        let input_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: subresource,
                },
            },
        };
        let mut input_view: Option<ID3D11VideoProcessorInputView> = None;
        unsafe {
            self.video_device.CreateVideoProcessorInputView(
                &decoder_texture,
                enumerator,
                &input_desc,
                Some(&mut input_view),
            )?
        };
        let input_view = input_view.context("创建 VideoProcessor input view 失败")?;

        let stream = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: BOOL(1),
            pInputSurface: ManuallyDrop::new(Some(input_view)),
            ..Default::default()
        };
        unsafe {
            self.video_context
                .VideoProcessorBlt(processor, output_view, 0, &[stream])
                .context("VideoProcessorBlt 失败")?;
        }

        let output_texture = self.output_texture.as_ref().context("DXVA 输出纹理缺失")?;
        let mut view: Option<ID3D11ShaderResourceView> = None;
        unsafe {
            self.device
                .CreateShaderResourceView(output_texture, None, Some(&mut view))?
        };
        view.context("DXVA 输出 SRV 创建失败")
    }
}

/// 先尝试 RGB32（无需转换），失败后退 NV12（CPU 转 BGRA）。
fn negotiate_video_output(reader: &IMFSourceReader, label: &str) -> Result<VideoPixelFormat> {
    let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
    let candidates = [
        (MFVideoFormat_RGB32, VideoPixelFormat::Bgra8, "RGB32"),
        (MFVideoFormat_NV12, VideoPixelFormat::Nv12, "NV12"),
    ];

    for (subtype, pixel_format, name) in candidates {
        let output_type: IMFMediaType = unsafe { MFCreateMediaType() }?;
        unsafe {
            output_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            output_type.SetGUID(&MF_MT_SUBTYPE, &subtype)?;
        }

        match unsafe { reader.SetCurrentMediaType(stream, None, &output_type) } {
            Ok(()) => {
                let negotiated = unsafe { reader.GetCurrentMediaType(stream)? };
                let actual = unsafe { negotiated.GetGUID(&MF_MT_SUBTYPE)? };
                if actual == subtype {
                    log::info!("[Video] 输出格式协商成功 file={} format={}", label, name);
                    return Ok(pixel_format);
                }
                log::debug!(
                    "[Video] 输出格式不符 file={} want={} got={:?}",
                    label,
                    name,
                    actual
                );
            }
            Err(err) => {
                log::debug!(
                    "[Video] 输出格式协商失败 file={} format={} err={:#}",
                    label,
                    name,
                    err
                );
            }
        }
    }

    Err(anyhow!(
        "视频解码器无法协商 RGB32 或 NV12 输出；当前仅支持这两种像素格式"
    ))
}

/// 打开失败时用来确认 SourceReader 实际能提供哪些输出 subtype。
fn log_native_video_subtypes(reader: &IMFSourceReader, label: &str) {
    let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
    let mut index = 0u32;
    loop {
        match unsafe { reader.GetNativeMediaType(stream, index) } {
            Ok(media_type) => {
                if let Ok(subtype) = unsafe { media_type.GetGUID(&MF_MT_SUBTYPE) } {
                    log::debug!(
                        "[Video] 原生输出 subtype[{}]={:?} file={}",
                        index,
                        subtype,
                        label
                    );
                }
                index = index.saturating_add(1);
            }
            Err(err) if err.code() == MF_E_NO_MORE_TYPES => break,
            Err(err) => {
                log::debug!(
                    "[Video] 枚举原生输出 subtype 失败 file={} err={:#}",
                    label,
                    err
                );
                break;
            }
        }
    }
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
    /// 同一个视频文件旁挂的音频；无音轨或打开失败时为 None，不影响视频链路。
    audio: Option<AudioPlayback>,
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
    /// 实际协商成功的解码输出格式；决定 upload_buffer 的像素路径。
    pixel_format: VideoPixelFormat,
    /// SourceReader 通过它使用 DXVA；必须比 reader 活得更久。
    _device_manager: Option<IMFDXGIDeviceManager>,
    /// DXVA sample 的 GPU 转换状态。
    video_processor: Option<VideoProcessorState>,
    /// DXVA buffer 是否出现过，只打印一次日志。
    hardware_sample_logged: bool,
    /// 日志只显示文件名，避免把绝对路径写进日志。
    label: String,
    timing: VideoTimingStats,
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
        volume: u32,
    ) -> Result<Self> {
        let started = Instant::now();
        let label = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("<unknown>")
            .to_string();
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
        let device_manager = create_dxgi_device_manager(&attributes, device, &label);
        let reader = unsafe { MFCreateSourceReaderFromURL(url, &attributes) }
            .with_context(|| format!("Media Foundation 无法打开视频 {:?}", path))?;

        // 关掉所有流再只开第一个视频流：视频壁纸不播音频。
        unsafe {
            reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
            reader.SetStreamSelection(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, true)?;
        }

        log_native_video_subtypes(&reader, &label);
        let pixel_format = negotiate_video_output(&reader, &label)?;

        // 音频默认暂停在 0；由 renderer 根据当前是否 active / fade / 全屏决定何时开始。
        // 打开失败只记日志，视频继续静音播放。
        let audio = AudioPlayback::open(path, volume);
        let video_processor = VideoProcessorState::new(device, context).ok();

        let mut playback = Self {
            reader,
            device: device.clone(),
            context: context.clone(),
            texture: None,
            audio,
            started_at: Instant::now(),
            paused_total: Duration::ZERO,
            paused_at: None,
            frame_time: Duration::ZERO,
            frame_duration: Duration::from_millis(40),
            cycle_base: Duration::ZERO,
            loop_duration: Duration::ZERO,
            pixel_format,
            _device_manager: device_manager,
            video_processor,
            hardware_sample_logged: false,
            label: label.clone(),
            timing: VideoTimingStats::default(),
        };

        // 先取一帧，确保打开阶段就能发现解码失败（错误需要在 set_wallpaper 时回传）。
        let first_frame_started = Instant::now();
        playback.read_next_frame()?;
        log::info!(
            "[Video] 打开完成 file={} open_ms={:.1} first_frame_ms={:.1}",
            label,
            started.elapsed().as_secs_f64() * 1000.0,
            first_frame_started.elapsed().as_secs_f64() * 1000.0
        );
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

        // 视频时钟是唯一基准；音频只做低频循环检测与漂移校准。
        if let Some(audio) = self.audio.as_mut() {
            audio.tick(self.frame_time, self.loop_duration, now);
        }
        self.timing.report_if_due(&self.label);
        Ok(())
    }

    /// crossfade 结束时调用：视频时间从 `now` 重新由 0 开始。
    ///
    /// fade 期间调用方不 advance，SourceReader 停在 open 时预读的首帧之后；
    /// 这里 flush + seek(0) 并重新预读，丢掉的只是那张预读帧，不会打断正在播放的内容。
    /// 不销毁 SourceReader，和暂停恢复保持同一套生命周期。
    pub fn reset_clock(&mut self, now: Instant) -> Result<()> {
        let reset_started = Instant::now();
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
        if let Some(audio) = self.audio.as_mut() {
            audio.reset();
        }
        let elapsed = reset_started.elapsed();
        self.timing.record_reset(elapsed);
        log::debug!(
            "[Video] 时钟重置 file={} reset_ms={:.1}",
            self.label,
            elapsed.as_secs_f64() * 1000.0
        );
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
        // 音频的暂停由 renderer 的 sync_audio_state 统一决定：
        // 全屏暂停、fade 冻结、pending 队列三种状态不能混用同一个开关。
    }

    /// fade 期间只冻结音频，不动视频时钟（视频冻结由 renderer 跳过 advance 实现）。
    pub fn set_audio_paused(&mut self, paused: bool) {
        if let Some(audio) = self.audio.as_mut() {
            audio.set_paused(paused);
        }
    }

    pub fn set_audio_volume(&mut self, volume: u32) {
        if let Some(audio) = self.audio.as_mut() {
            audio.set_volume(volume);
        }
    }

    /// 当前帧的 SRV 与尺寸（SRV 是 COM 引用计数，clone 很便宜）。
    /// 返回 owned 值，避免跨 MutexGuard 暴露借用。
    pub fn frame_snapshot(&self) -> Option<VideoFrameSnapshot> {
        self.texture.as_ref().map(|t| VideoFrameSnapshot {
            view: t.view.clone(),
            uv_view: t.uv_view.clone(),
            width: t.width,
            height: t.height,
        })
    }

    /// 读取下一帧并上传到复用纹理；返回 false 表示 EOF。
    fn read_next_frame(&mut self) -> Result<bool> {
        let read_started = Instant::now();
        let mut flags = 0u32;
        let mut timestamp = 0i64;
        let mut sample: Option<IMFSample> = None;
        unsafe {
            self.reader
                .ReadSample(
                    MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                    0,
                    None,
                    Some(&mut flags),
                    Some(&mut timestamp),
                    Some(&mut sample),
                )
                .context("ReadSample 读取视频帧失败")?;
        }

        if (flags as i32) & MF_SOURCE_READERF_ENDOFSTREAM.0 != 0 || sample.is_none() {
            return Ok(false);
        }
        let sample = sample.context("SourceReader 返回空 sample")?;

        log::debug!(
            "[Video] 首帧 sample 已读取 file={} flags={} timestamp={}",
            self.label,
            flags,
            timestamp
        );

        let duration = unsafe { sample.GetSampleDuration() }.unwrap_or(0);
        let frame_duration = hns_to_duration(duration.max(0) as u64);
        let frame_time = self
            .cycle_base
            .saturating_add(hns_to_duration(timestamp.max(0) as u64));

        // 只在首轮估算循环时长；后续轮次 cycle_base 已承接，不需要重算。
        if self.cycle_base.is_zero() {
            self.loop_duration = frame_time.saturating_add(frame_duration);
        }

        // DXVA sample 走 VideoProcessor：GPU 解码纹理 → shader 可采样的 BGRA8 纹理。
        let dxgi_buffer = unsafe { sample.GetBufferByIndex(0) }
            .ok()
            .and_then(|buffer| buffer.cast::<IMFDXGIBuffer>().ok());
        if let Some(dxgi_buffer) = dxgi_buffer {
            if !self.hardware_sample_logged {
                self.hardware_sample_logged = true;
                log::info!("[Video] 使用 DXVA GPU sample file={}", self.label);
            }

            let media_type = unsafe {
                self.reader
                    .GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)?
            };
            let frame_size = unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE)? };
            let width = (frame_size >> 32) as u32;
            let height = frame_size as u32;
            if width == 0 || height == 0 {
                return Err(anyhow!("DXVA sample 帧尺寸无效: {}x{}", width, height));
            }

            let state = self
                .video_processor
                .as_mut()
                .context("检测到 DXVA sample，但 VideoProcessor 不可用")?;
            let view = state
                .process(&dxgi_buffer, width, height)
                .context("DXVA VideoProcessor 处理失败")?;
            let output_texture = state
                .output_texture
                .as_ref()
                .context("DXVA 输出纹理缺失")?
                .clone();

            self.texture = Some(FrameTexture {
                view,
                texture: output_texture,
                uv_view: None,
                uv_texture: None,
                pixel_format: VideoPixelFormat::Bgra8,
                width,
                height,
            });
            self.frame_time = frame_time;
            self.frame_duration = if frame_duration.is_zero() {
                Duration::from_millis(40)
            } else {
                frame_duration
            };
            self.timing
                .record_frame(read_started.elapsed(), Duration::ZERO);
            return Ok(true);
        }

        let buffer = unsafe { sample.ConvertToContiguousBuffer() }
            .context("IMFSample 转为连续缓冲区失败")?;
        let read_elapsed = read_started.elapsed();
        log::debug!("[Video] sample 已转连续缓冲区 file={}", self.label);

        let upload_started = Instant::now();
        log::debug!("[Video] 开始上传视频帧 file={}", self.label);
        self.upload_buffer(&buffer)?;
        log::debug!("[Video] 视频帧上传完成 file={}", self.label);
        self.timing
            .record_frame(read_elapsed, upload_started.elapsed());

        self.frame_time = frame_time;
        self.frame_duration = if frame_duration.is_zero() {
            Duration::from_millis(40)
        } else {
            frame_duration
        };
        Ok(true)
    }

    /// 把 RGB32 / NV12 缓冲区上传到一张复用的 BGRA8 D3D11 纹理。
    fn upload_buffer(&mut self, buffer: &IMFMediaBuffer) -> Result<()> {
        use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;

        let mut ptr: *mut u8 = ptr::null_mut();
        let mut max_len = 0u32;
        let mut current_len = 0u32;
        unsafe {
            buffer
                .Lock(&mut ptr, Some(&mut max_len), Some(&mut current_len))
                .context("锁定视频 sample 缓冲区失败")?
        };
        let result = (|| -> Result<()> {
            if ptr.is_null() {
                return Err(anyhow!("Video sample 缓冲区指针为空"));
            }
            let buffer_len = max_len.max(current_len) as usize;
            log::debug!(
                "[Video] 缓冲区已锁定 file={} max={} current={}",
                self.label,
                max_len,
                current_len
            );
            let media_type = unsafe {
                self.reader
                    .GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)
                    .context("读取当前视频媒体类型失败")?
            };
            let frame_size = unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE) }
                .context("读取视频帧尺寸 MF_MT_FRAME_SIZE 失败")?;
            let width = (frame_size >> 32) as u32;
            let height = frame_size as u32;
            if width == 0 || height == 0 {
                return Err(anyhow!("视频帧尺寸无效: {}x{}", width, height));
            }

            let row_bytes = match self.pixel_format {
                VideoPixelFormat::Bgra8 => width as usize * 4,
                VideoPixelFormat::Nv12 => width as usize,
            };
            // NV12 的 MF_MT_DEFAULT_STRIDE 不一定存在；缺失时按 NV12 平面总字节数反推，
            // 比直接取 width 更稳：解码器常把 stride 对齐到 64/128 字节。
            let raw_stride =
                unsafe { media_type.GetUINT64(&MF_MT_DEFAULT_STRIDE) }.unwrap_or(0) as i64;
            let stride = if raw_stride != 0 {
                raw_stride.unsigned_abs() as usize
            } else if self.pixel_format == VideoPixelFormat::Nv12 {
                let planes = height as usize + height as usize / 2;
                (buffer_len / planes.max(1)).max(row_bytes)
            } else {
                row_bytes
            };
            let bottom_up = raw_stride < 0;
            if stride < row_bytes {
                return Err(anyhow!(
                    "视频 stride({}) 小于行字节数({})，拒绝上传避免越界读取",
                    stride,
                    row_bytes
                ));
            }
            if self.pixel_format == VideoPixelFormat::Nv12 && bottom_up {
                return Err(anyhow!("NV12 bottom-up 布局暂不支持"));
            }
            log::debug!(
                "[Video] 帧参数 file={} format={:?} {}x{} stride={} buffer={}",
                self.label,
                self.pixel_format,
                width,
                height,
                stride,
                buffer_len
            );

            self.ensure_texture(width, height)?;
            let texture: ID3D11Texture2D =
                self.texture.as_ref().expect("纹理已就绪").texture.clone();
            let expected = width as usize * 4;

            match self.pixel_format {
                VideoPixelFormat::Bgra8 => {
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
                    }
                    self.copy_rows(
                        &texture,
                        ptr as *const u8,
                        stride,
                        height as usize,
                        expected,
                        bottom_up,
                    )?;
                }
                VideoPixelFormat::Nv12 => {
                    if width % 2 != 0 || height % 2 != 0 {
                        return Err(anyhow!("NV12 宽高必须为偶数: {}x{}", width, height));
                    }
                    let uv_offset = stride
                        .checked_mul(height as usize)
                        .ok_or_else(|| anyhow!("NV12 Y 平面偏移溢出"))?;
                    let uv_bytes = stride
                        .checked_mul(height as usize / 2)
                        .ok_or_else(|| anyhow!("NV12 UV 平面字节数溢出"))?;
                    let needed = uv_offset
                        .checked_add(uv_bytes)
                        .ok_or_else(|| anyhow!("NV12 平面总字节数溢出"))?;
                    if buffer_len < needed {
                        return Err(anyhow!(
                            "NV12 sample 数据不足: 需要 {} 字节，实际 {}",
                            needed,
                            buffer_len
                        ));
                    }

                    let uv_texture = self
                        .texture
                        .as_ref()
                        .and_then(|t| t.uv_texture.clone())
                        .context("NV12 UV 纹理不存在")?;
                    self.copy_rows(
                        &texture,
                        ptr as *const u8,
                        stride,
                        height as usize,
                        width as usize,
                        false,
                    )?;
                    self.copy_rows(
                        &uv_texture,
                        unsafe { ptr.add(uv_offset) } as *const u8,
                        stride,
                        height as usize / 2,
                        width as usize,
                        false,
                    )?;
                }
            }
            log::debug!("[Video] 纹理行上传完成 file={}", self.label);
            Ok(())
        })();
        unsafe { buffer.Unlock()? };
        result
    }

    fn ensure_texture(&mut self, width: u32, height: u32) -> Result<()> {
        use windows::Win32::Graphics::Direct3D11::{
            D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_WRITE, D3D11_TEXTURE2D_DESC,
            D3D11_USAGE_DYNAMIC,
        };
        use windows::Win32::Graphics::Dxgi::Common::{
            DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R8G8_UNORM, DXGI_FORMAT_R8_UNORM,
            DXGI_SAMPLE_DESC,
        };

        let need_rebuild = self
            .texture
            .as_ref()
            .map(|t| t.width != width || t.height != height || t.pixel_format != self.pixel_format)
            .unwrap_or(true);
        if !need_rebuild {
            return Ok(());
        }

        let make_view = |texture: &ID3D11Texture2D| -> Result<ID3D11ShaderResourceView> {
            let mut view: Option<ID3D11ShaderResourceView> = None;
            unsafe {
                self.device
                    .CreateShaderResourceView(texture, None, Some(&mut view))?
            };
            view.context("视频帧 SRV 创建失败")
        };

        let desc = |tex_width: u32, tex_height: u32, format| D3D11_TEXTURE2D_DESC {
            Width: tex_width,
            Height: tex_height,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            MiscFlags: 0,
        };
        let create = |d: &D3D11_TEXTURE2D_DESC| -> Result<ID3D11Texture2D> {
            let mut texture: Option<ID3D11Texture2D> = None;
            unsafe { self.device.CreateTexture2D(d, None, Some(&mut texture))? };
            texture.context("视频帧纹理创建失败")
        };

        let (view, texture, uv_view, uv_texture) = match self.pixel_format {
            VideoPixelFormat::Bgra8 => {
                let texture = create(&desc(width, height, DXGI_FORMAT_B8G8R8A8_UNORM))?;
                let view = make_view(&texture)?;
                (view, texture, None, None)
            }
            VideoPixelFormat::Nv12 => {
                // NV12 的 UV 是 width/2 × height/2 个 R8G8 texel，不是 width × height/2。
                let y_texture = create(&desc(width, height, DXGI_FORMAT_R8_UNORM))?;
                let uv_texture = create(&desc(width / 2, height / 2, DXGI_FORMAT_R8G8_UNORM))?;
                let y_view = make_view(&y_texture)?;
                let uv_view = make_view(&uv_texture)?;
                (y_view, y_texture, Some(uv_view), Some(uv_texture))
            }
        };

        self.texture = Some(FrameTexture {
            view,
            texture,
            uv_view,
            uv_texture,
            pixel_format: self.pixel_format,
            width,
            height,
        });
        Ok(())
    }

    fn copy_rows(
        &self,
        texture: &ID3D11Texture2D,
        source: *const u8,
        source_stride: usize,
        rows: usize,
        row_bytes: usize,
        reverse: bool,
    ) -> Result<()> {
        use windows::Win32::Graphics::Direct3D11::{
            D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_WRITE_DISCARD,
        };

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context
                .Map(texture, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))
                .context("映射视频纹理失败")?;
        }
        let copy_result = (|| -> Result<()> {
            if mapped.pData.is_null() {
                return Err(anyhow!("视频纹理 Map 返回空指针"));
            }
            let dst_stride = mapped.RowPitch as usize;
            if dst_stride < row_bytes {
                return Err(anyhow!(
                    "视频纹理 RowPitch({}) 小于行字节数({})",
                    dst_stride,
                    row_bytes
                ));
            }
            let dst_base = mapped.pData as *mut u8;
            for row in 0..rows {
                let src_row = if reverse { rows - 1 - row } else { row };
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        source.add(src_row * source_stride),
                        dst_base.add(row * dst_stride),
                        row_bytes,
                    );
                }
            }
            Ok(())
        })();
        unsafe { self.context.Unmap(texture, 0) };
        copy_result
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
