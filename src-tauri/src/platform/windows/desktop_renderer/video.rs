//! DXVA 硬件解码视频链路：解码与 GPU 处理隔离到独立 worker，renderer 只读共享纹理。
//!
//! 为什么单独 worker：MF 解码器、SourceReader、device manager 会在自己的线程占用 D3D11
//! device；renderer 直接消费 DXVA sample 会争抢同一个 device/context，表现为 device removed
//! 或线程冻结。这里让 worker 独占解码设备，VideoProcessor 输出到 keyed-mutex 共享纹理，
//! renderer 只打开该纹理并短暂持有 mutex 绘制。

use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{self, NonNull};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use windows::core::{Interface, BOOL, GUID};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D10::ID3D10Multithread;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView,
    ID3D11Texture2D, ID3D11VideoContext, ID3D11VideoDevice, ID3D11VideoProcessor,
    ID3D11VideoProcessorEnumerator, ID3D11VideoProcessorInputView, ID3D11VideoProcessorOutputView,
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX, D3D11_SDK_VERSION,
    D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
    D3D11_VPIV_DIMENSION_TEXTURE2D, D3D11_VPOV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{IDXGIAdapter, IDXGIDevice, IDXGIKeyedMutex, IDXGIResource};
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFDXGIBuffer, IMFDXGIDeviceManager, IMFMediaType, IMFSample, IMFSourceReader,
    MFCreateAttributes, MFCreateDXGIDeviceManager, MFCreateMediaType, MFCreateSourceReaderFromURL,
    MFMediaType_Video, MFStartup, MFVideoFormat_NV12, MFSTARTUP_FULL, MF_MT_FRAME_SIZE,
    MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS,
    MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED, MF_SOURCE_READERF_ENDOFSTREAM,
    MF_SOURCE_READER_ALL_STREAMS, MF_SOURCE_READER_D3D_MANAGER, MF_SOURCE_READER_DISABLE_DXVA,
    MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_VERSION,
};

use super::audio::AudioPlayback;

pub fn startup() -> Result<()> {
    unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.context("Media Foundation 初始化失败")
}

/// renderer 绘制一帧视频所需的资源。keyed_mutex 在绘制后必须 release。
pub struct VideoFrameSnapshot {
    pub view: ID3D11ShaderResourceView,
    pub uv_view: Option<ID3D11ShaderResourceView>,
    pub width: u32,
    pub height: u32,
    keyed_mutex: Option<IDXGIKeyedMutex>,
}

impl VideoFrameSnapshot {
    pub fn for_static_frame(view: ID3D11ShaderResourceView, width: u32, height: u32) -> Self {
        Self {
            view,
            uv_view: None,
            width,
            height,
            keyed_mutex: None,
        }
    }

    pub fn release(&self) {
        if let Some(keyed_mutex) = &self.keyed_mutex {
            let _ = unsafe { keyed_mutex.ReleaseSync(0) };
        }
    }
}

#[derive(Clone, Default)]
struct SharedState {
    frame_time: Duration,
    frame_duration: Duration,
    loop_duration: Duration,
    generation: u64,
    ready: bool,
}

#[derive(Clone)]
struct SharedFrameInfo {
    handle: isize,
    width: u32,
    height: u32,
}

enum WorkerCommand {
    Exit,
    Pause {
        paused: bool,
        now: Instant,
    },
    Reset {
        now: Instant,
        ack: Sender<Result<()>>,
    },
}

enum WorkerEvent {
    Frame(SharedFrameInfo),
}

pub struct VideoPlayback {
    worker: Option<JoinHandle<()>>,
    commands: Sender<WorkerCommand>,
    state: Arc<Mutex<SharedState>>,
    frame: Option<VideoFrameSnapshot>,
    audio: Option<AudioPlayback>,
    started_at: Instant,
    paused_total: Duration,
    paused_at: Option<Instant>,
    frame_time: Duration,
    frame_duration: Duration,
}

impl VideoPlayback {
    pub fn open(
        renderer_device: &ID3D11Device,
        _renderer_context: &ID3D11DeviceContext,
        path: &Path,
        volume: u32,
    ) -> Result<Self> {
        let label = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("<unknown>")
            .to_string();
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(SharedState::default()));
        let worker_state = state.clone();
        let worker_label = label.clone();
        let worker_path = path.to_path_buf();
        let worker_renderer_device = renderer_device.clone();

        let worker = thread::Builder::new()
            .name("wallpaper-dxva".to_string())
            .spawn(move || {
                if let Err(err) = worker_main(
                    &worker_renderer_device,
                    &worker_path,
                    &worker_label,
                    command_rx,
                    event_tx,
                    worker_state,
                ) {
                    log::error!("[VideoHw] worker 退出: {:#}", err);
                }
            })
            .context("DXVA worker 线程创建失败")?;

        let frame_info = event_rx
            .recv_timeout(Duration::from_secs(10))
            .context("等待 DXVA 首帧共享纹理超时")?;
        let WorkerEvent::Frame(info) = frame_info;

        let frame = open_shared_frame(renderer_device, &info)?;
        let audio = AudioPlayback::open(path, volume);

        log::info!(
            "[VideoHw] DXVA worker 就绪 file={} {}x{}",
            label,
            info.width,
            info.height
        );
        Ok(Self {
            worker: Some(worker),
            commands: command_tx,
            state,
            frame: Some(frame),
            audio,
            started_at: Instant::now(),
            paused_total: Duration::ZERO,
            paused_at: None,
            frame_time: Duration::ZERO,
            frame_duration: Duration::from_millis(40),
        })
    }

    pub fn advance(&mut self, now: Instant) -> Result<()> {
        if self.paused_at.is_some() {
            return Ok(());
        }
        let shared = self.state.lock().unwrap().clone();
        if shared.ready {
            self.frame_time = shared.frame_time;
            self.frame_duration = shared.frame_duration;
            if let Some(audio) = self.audio.as_mut() {
                audio.tick(shared.frame_time, shared.loop_duration, now);
            }
        }
        Ok(())
    }

    pub fn reset_clock(&mut self, now: Instant) -> Result<()> {
        let (ack_tx, ack_rx) = mpsc::channel();
        self.commands
            .send(WorkerCommand::Reset { now, ack: ack_tx })
            .context("DXVA worker 命令通道关闭")?;
        ack_rx
            .recv_timeout(Duration::from_secs(3))
            .context("等待 DXVA worker 重置超时")??;
        self.started_at = now;
        self.paused_total = Duration::ZERO;
        self.paused_at = None;
        if let Some(audio) = self.audio.as_mut() {
            audio.reset();
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
            _ => return,
        }
        let _ = self.commands.send(WorkerCommand::Pause { paused, now });
    }

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

    pub fn frame_snapshot(&self) -> Option<VideoFrameSnapshot> {
        let frame = self.frame.as_ref()?;
        let keyed_mutex = frame.keyed_mutex.as_ref()?;
        if unsafe { keyed_mutex.AcquireSync(1, 0) }.is_err() {
            return None;
        }
        Some(VideoFrameSnapshot {
            view: frame.view.clone(),
            uv_view: None,
            width: frame.width,
            height: frame.height,
            keyed_mutex: Some(keyed_mutex.clone()),
        })
    }

    // 仅用于让 advance 里的时间字段有初值，音频 tick 依赖它。
    #[allow(dead_code)]
    fn frame_time_placeholder(&self) {}
}

impl Drop for VideoPlayback {
    fn drop(&mut self) {
        let _ = self.commands.send(WorkerCommand::Exit);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn open_shared_frame(device: &ID3D11Device, info: &SharedFrameInfo) -> Result<VideoFrameSnapshot> {
    let mut texture: Option<ID3D11Texture2D> = None;
    let handle = HANDLE(info.handle as *mut c_void);
    unsafe { device.OpenSharedResource(handle, &mut texture)? };
    let texture = texture.context("打开 DXVA 共享纹理失败")?;
    let keyed_mutex: IDXGIKeyedMutex = texture.cast()?;
    let mut view: Option<ID3D11ShaderResourceView> = None;
    unsafe { device.CreateShaderResourceView(&texture, None, Some(&mut view))? };
    let view = view.context("DXVA 共享纹理 SRV 创建失败")?;
    Ok(VideoFrameSnapshot {
        view,
        uv_view: None,
        width: info.width,
        height: info.height,
        keyed_mutex: Some(keyed_mutex),
    })
}

fn worker_main(
    renderer_device: &ID3D11Device,
    path: &Path,
    label: &str,
    commands: Receiver<WorkerCommand>,
    events: Sender<WorkerEvent>,
    state: Arc<Mutex<SharedState>>,
) -> Result<()> {
    let mut events = Some(events);
    let (device, context) = create_worker_device(renderer_device)?;
    let mut pipeline = DxvaPipeline::new(&device, &context, path, label)?;
    let mut started = Instant::now();
    let mut paused_total = Duration::ZERO;
    let mut paused_at: Option<Instant> = None;
    let mut cycle_base = Duration::ZERO;
    let mut pending: Option<DecodedSample> = None;

    loop {
        while let Ok(command) = commands.try_recv() {
            match command {
                WorkerCommand::Exit => return Ok(()),
                WorkerCommand::Pause { paused, now } => match (paused, paused_at) {
                    (true, None) => paused_at = Some(now),
                    (false, Some(at)) => {
                        paused_total =
                            paused_total.saturating_add(now.saturating_duration_since(at));
                        paused_at = None;
                    }
                    _ => {}
                },
                WorkerCommand::Reset { now, ack } => {
                    let result = pipeline.reset();
                    if result.is_ok() {
                        started = now;
                        paused_total = Duration::ZERO;
                        paused_at = None;
                        cycle_base = Duration::ZERO;
                        pending = None;
                        let mut shared = state.lock().unwrap();
                        shared.frame_time = Duration::ZERO;
                        shared.frame_duration = Duration::from_millis(40);
                        shared.loop_duration = Duration::ZERO;
                        shared.generation = shared.generation.wrapping_add(1);
                        shared.ready = false;
                    }
                    let _ = ack.send(result);
                }
            }
        }

        if paused_at.is_some() {
            thread::sleep(Duration::from_millis(10));
            continue;
        }

        let target = started.elapsed().saturating_sub(paused_total);
        {
            let shared = state.lock().unwrap();
            if shared.ready && target < shared.frame_time.saturating_add(shared.frame_duration) {
                drop(shared);
                thread::sleep(Duration::from_millis(1));
                continue;
            }
        }

        let sample = match pending.take() {
            Some(sample) => sample,
            None => match pipeline.read_sample()? {
                Some(sample) => sample,
                None => {
                    cycle_base = cycle_base.saturating_add(pipeline.loop_duration());
                    pipeline.seek_zero()?;
                    continue;
                }
            },
        };

        // 共享纹理在 read_sample() 中已创建；立刻通知 renderer，避免首帧 VideoProcessor
        // 尚未完成时 open() 卡在等待事件上。renderer 在 state.ready 之前不会拿到帧。
        if let Some(sender) = events.take() {
            if sender
                .send(WorkerEvent::Frame(pipeline.shared_frame_info().clone()))
                .is_err()
            {
                return Ok(());
            }
        }

        match pipeline.process_sample(&sample) {
            Ok(true) => {
                let frame_time = cycle_base.saturating_add(sample.timestamp);
                pipeline.note_frame(frame_time, sample.duration, cycle_base.is_zero());
                let mut shared = state.lock().unwrap();
                shared.frame_time = frame_time;
                shared.frame_duration = sample.duration;
                shared.loop_duration = pipeline.loop_duration();
                shared.ready = true;
                drop(shared);
                log::debug!(
                    "[VideoHw] 首帧已提交 file={} frame_ms={:.1}",
                    label,
                    frame_time.as_secs_f64() * 1000.0
                );
            }
            Ok(false) => {
                pending = Some(sample);
                thread::sleep(Duration::from_millis(1));
            }
            Err(err) => return Err(err),
        }
    }
}

fn create_worker_device(
    renderer_device: &ID3D11Device,
) -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    let dxgi_device: IDXGIDevice = renderer_device.cast()?;
    let adapter: IDXGIAdapter = unsafe { dxgi_device.GetAdapter()? };
    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;
    unsafe {
        D3D11CreateDevice(
            Some(&adapter),
            D3D_DRIVER_TYPE_UNKNOWN,
            windows::Win32::Foundation::HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }
    let device = device.context("DXVA worker device 创建失败")?;
    let context = context.context("DXVA worker context 创建失败")?;
    // 微软文档明确建议给 MF/D3D11 共用 device 打开多线程保护，避免 VideoContext 死锁。
    if let Ok(multithread) = device.cast::<ID3D10Multithread>() {
        let _ = unsafe { multithread.SetMultithreadProtected(true) };
    } else {
        log::warn!("[VideoHw] ID3D10Multithread 不可用，跳过多线程保护");
    }
    Ok((device, context))
}

struct DecodedSample {
    sample: IMFSample,
    timestamp: Duration,
    duration: Duration,
}

impl DecodedSample {
    fn new(sample: IMFSample, timestamp: i64, duration: i64) -> Self {
        Self {
            sample,
            timestamp: hns_to_duration(timestamp.max(0) as u64),
            duration: normalize_duration(hns_to_duration(duration.max(0) as u64)),
        }
    }
}

struct DxvaPipeline {
    reader: IMFSourceReader,
    manager: IMFDXGIDeviceManager,
    manager_handle: HANDLE,
    device: ID3D11Device,
    video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,
    enumerator: Option<ID3D11VideoProcessorEnumerator>,
    processor: Option<ID3D11VideoProcessor>,
    shared_texture: Option<ID3D11Texture2D>,
    shared_info: Option<SharedFrameInfo>,
    output_view: Option<ID3D11VideoProcessorOutputView>,
    width: u32,
    height: u32,
    loop_duration: Duration,
    label: String,
    first_frame_logged: bool,
}

impl DxvaPipeline {
    fn new(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        path: &Path,
        label: &str,
    ) -> Result<Self> {
        let mut reset_token = 0u32;
        let mut manager: Option<IMFDXGIDeviceManager> = None;
        unsafe { MFCreateDXGIDeviceManager(&mut reset_token, &mut manager)? };
        let manager = manager.context("IMFDXGIDeviceManager 创建失败")?;
        unsafe {
            manager.ResetDevice(device, reset_token)?;
        }

        let attributes: IMFAttributes = unsafe {
            let mut value = None;
            MFCreateAttributes(&mut value, 3)?;
            value.context("MF attributes 创建失败")?
        };
        unsafe {
            attributes.SetUnknown(&MF_SOURCE_READER_D3D_MANAGER, &manager)?;
            attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
            attributes.SetUINT32(&MF_SOURCE_READER_DISABLE_DXVA, 0)?;
        }

        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let reader = unsafe {
            MFCreateSourceReaderFromURL(windows::core::PCWSTR(wide.as_ptr()), &attributes)
        }
        .with_context(|| format!("SourceReader 无法打开 {:?}", path))?;
        unsafe {
            reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
            reader.SetStreamSelection(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, true)?;
        }
        let output_type: IMFMediaType = unsafe { MFCreateMediaType() }?;
        unsafe {
            output_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            output_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
            reader.SetCurrentMediaType(
                MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                None,
                &output_type,
            )?;
        }
        let manager_handle = unsafe { manager.OpenDeviceHandle() }?;
        // 微软文档：GetVideoService 只用于取 ID3D11Device / ID3D11VideoDevice，
        // ID3D11VideoContext 必须从 ID3D11DeviceContext QueryInterface。
        let video_context = match (|| -> Result<ID3D11VideoContext> {
            let mut device_raw: *mut c_void = ptr::null_mut();
            unsafe {
                manager.GetVideoService(manager_handle, &ID3D11Device::IID, &mut device_raw)?;
            }
            let device_raw = NonNull::new(device_raw).context("GetVideoService 返回空 device")?;
            let manager_device: ID3D11Device =
                unsafe { ID3D11Device::from_raw(device_raw.as_ptr()) };
            let manager_context = unsafe { manager_device.GetImmediateContext()? };
            Ok(manager_context.cast()?)
        })() {
            Ok(value) => value,
            Err(err) => {
                log::warn!(
                    "[VideoHw] device manager 取 VideoContext 失败，改用 worker context: {:#}",
                    err
                );
                context.cast()?
            }
        };
        log::info!("[VideoHw] VideoContext 就绪 file={}", label);
        Ok(Self {
            manager,
            manager_handle,
            reader,
            device: device.clone(),
            video_device: device.cast()?,
            video_context,
            enumerator: None,
            processor: None,
            shared_texture: None,
            shared_info: None,
            output_view: None,
            width: 0,
            height: 0,
            loop_duration: Duration::ZERO,
            label: label.to_string(),
            first_frame_logged: false,
        })
    }

    fn read_sample(&mut self) -> Result<Option<DecodedSample>> {
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
            return Ok(None);
        }
        let sample = sample.context("SourceReader 返回空 sample")?;
        let duration = unsafe { sample.GetSampleDuration() }.unwrap_or(0);
        let decoded = DecodedSample::new(sample, timestamp, duration);
        if (flags as i32) & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 != 0 || self.width == 0 {
            let media_type = unsafe {
                self.reader
                    .GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)?
            };
            let frame_size = unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE)? };
            let width = (frame_size >> 32) as u32;
            let height = frame_size as u32;
            self.ensure_output(width, height)?;
        }
        Ok(Some(decoded))
    }

    fn ensure_output(&mut self, width: u32, height: u32) -> Result<()> {
        if self.width == width && self.height == height && self.output_view.is_some() {
            return Ok(());
        }
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
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX.0 as u32,
        };
        let mut texture: Option<ID3D11Texture2D> = None;
        unsafe {
            self.device
                .CreateTexture2D(&desc, None, Some(&mut texture))?
        };
        let texture = texture.context("共享视频纹理创建失败")?;
        let resource: IDXGIResource = texture.cast()?;
        let handle = unsafe { resource.GetSharedHandle() }?.0 as isize;

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
        let enumerator = unsafe { self.video_device.CreateVideoProcessorEnumerator(&content) }?;
        let processor = unsafe { self.video_device.CreateVideoProcessor(&enumerator, 0) }?;
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
            )?;
        }
        let output_view = output_view.context("VideoProcessor output view 创建失败")?;

        let keyed_mutex: IDXGIKeyedMutex = texture.cast()?;
        unsafe { keyed_mutex.AcquireSync(0, 0)? };
        let _ = unsafe { keyed_mutex.ReleaseSync(1) };

        self.width = width;
        self.height = height;
        self.shared_texture = Some(texture);
        self.shared_info = Some(SharedFrameInfo {
            handle,
            width,
            height,
        });
        self.enumerator = Some(enumerator);
        self.processor = Some(processor);
        self.output_view = Some(output_view);
        log::info!(
            "[VideoHw] 共享纹理就绪 file={} {}x{}",
            self.label,
            width,
            height
        );
        Ok(())
    }

    fn process_sample(&mut self, decoded: &DecodedSample) -> Result<bool> {
        let output_view = self.output_view.clone().context("VideoProcessor 未就绪")?;
        let processor = self.processor.clone().context("VideoProcessor 未就绪")?;
        let enumerator = self
            .enumerator
            .clone()
            .context("VideoProcessor enumerator 未就绪")?;

        let buffer = unsafe { decoded.sample.GetBufferByIndex(0) }?;
        let dxgi_buffer: IMFDXGIBuffer = buffer.cast()?;
        let mut raw: *mut c_void = ptr::null_mut();
        unsafe { dxgi_buffer.GetResource(&ID3D11Texture2D::IID, &mut raw)? };
        let raw = NonNull::new(raw).context("DXVA sample 返回空 texture")?;
        let decoder_texture: ID3D11Texture2D = unsafe { ID3D11Texture2D::from_raw(raw.as_ptr()) };
        let subresource = unsafe { dxgi_buffer.GetSubresourceIndex() }?;

        // VideoContext 在 pipeline 初始化时通过 immediate context QueryInterface 得到。
        // 不能逐帧调用 GetVideoService(IID_ID3D11VideoContext)，该 IID 不受支持。
        let video_context = self.video_context.clone();
        let _ = unsafe { self.manager.TestDevice(self.manager_handle) };

        let shared_texture = self.shared_texture.clone().context("共享纹理缺失")?;
        let keyed_mutex: IDXGIKeyedMutex = shared_texture.cast()?;
        unsafe { keyed_mutex.AcquireSync(0, 250)? };
        if !self.first_frame_logged {
            log::info!("[VideoHw] 共享纹理 key0 已获取 file={}", self.label);
        }

        let result = (|| -> Result<()> {
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
                    &enumerator,
                    &input_desc,
                    Some(&mut input_view),
                )?;
            }
            let input_view = input_view.context("VideoProcessor input view 创建失败")?;
            let stream = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: BOOL(1),
                pInputSurface: ManuallyDrop::new(Some(input_view)),
                ..Default::default()
            };
            if !self.first_frame_logged {
                log::info!("[VideoHw] 开始首帧 VideoProcessorBlt file={}", self.label);
            }
            unsafe {
                video_context.VideoProcessorBlt(&processor, &output_view, 0, &[stream])?;
            }
            Ok(())
        })();
        let _ = unsafe { keyed_mutex.ReleaseSync(1) };
        if result.is_ok() && !self.first_frame_logged {
            self.first_frame_logged = true;
            log::info!("[VideoHw] 首帧 VideoProcessorBlt 完成 file={}", self.label);
        }
        result.map(|_| true)
    }

    fn shared_frame_info(&self) -> &SharedFrameInfo {
        self.shared_info.as_ref().expect("共享纹理信息已就绪")
    }

    fn note_frame(&mut self, frame_time: Duration, duration: Duration, first_cycle: bool) {
        if first_cycle && self.loop_duration.is_zero() {
            self.loop_duration = frame_time.saturating_add(duration);
        }
    }

    fn loop_duration(&self) -> Duration {
        self.loop_duration
    }

    fn seek_zero(&mut self) -> Result<()> {
        let position = seek_zero_position();
        unsafe {
            self.reader
                .Flush(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)?;
            self.reader
                .SetCurrentPosition(&GUID::default(), &position)?;
        }
        Ok(())
    }

    fn reset(&mut self) -> Result<()> {
        self.loop_duration = Duration::ZERO;
        self.seek_zero()
    }
}

fn seek_zero_position() -> windows::Win32::System::Com::StructuredStorage::PROPVARIANT {
    use windows::Win32::System::Com::StructuredStorage::{
        PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
    };
    use windows::Win32::System::Variant::VT_I8;

    PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_I8,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: PROPVARIANT_0_0_0 { hVal: 0 },
            }),
        },
    }
}

fn hns_to_duration(hns: u64) -> Duration {
    Duration::from_nanos((hns as u128).saturating_mul(100).min(u64::MAX as u128) as u64)
}

fn normalize_duration(duration: Duration) -> Duration {
    if duration.is_zero() {
        Duration::from_millis(40)
    } else {
        duration
    }
}
