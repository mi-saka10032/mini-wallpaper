//! Native 视频的旁挂音频播放（Windows 专用）。
//!
//! 视频帧由 `SourceReader` 拉取并上传 D3D11；这里同样用 `SourceReader` 拉取音频流，
//! 解码为 PCM 后经 WASAPI shared-mode 输出。没有引入 WinRT MediaPlayer / MediaSession，
//! 因此音视频链路都属于同一套 Media Foundation 拉取模型：解码器由系统提供，
//! 缓冲、时钟基准、seek、暂停、音量与循环控制都在我们自己的代码里。
//!
//! 音频线程独立运行，避免 `ReadSample` 阻塞 renderer 线程。音频位置以 100ns 原子值
//! 暴露给 renderer，仅用于 5s 低频校准；视频时钟仍是唯一基准。

use std::collections::VecDeque;
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use windows::core::GUID;
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::Media::Audio::{
    eMultimedia, eRender, IAudioClient, IAudioRenderClient, IMMDeviceEnumerator,
    ISimpleAudioVolume, MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED, WAVEFORMATEX,
};
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFMediaType, IMFSample, IMFSourceReader, MFAudioFormat_Float,
    MFAudioFormat_PCM, MFCreateAttributes, MFCreateMediaType, MFCreateSourceReaderFromURL,
    MFMediaType_Audio, MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MF_MT_AUDIO_BITS_PER_SAMPLE,
    MF_MT_AUDIO_BLOCK_ALIGNMENT, MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND,
    MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READER_ALL_STREAMS,
    MF_SOURCE_READER_FIRST_AUDIO_STREAM,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0},
    CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::System::Variant::VT_I8;

// 只依赖 windows-core，不需要启用 KernelStreaming / Multimedia 模块。
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID =
    GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);

const DRIFT_CHECK_INTERVAL: Duration = Duration::from_secs(5);
const DRIFT_THRESHOLD: Duration = Duration::from_millis(100);
const MAX_RING_BYTES: usize = 1 << 20;
const AUDIO_BUFFER_DURATION_HNS: i64 = 2_000_000;

enum AudioCommand {
    Pause,
    Play,
    Seek(Duration),
    SetVolume(u32),
    Shutdown,
}

/// 只向 renderer 暴露控制面；COM / SourceReader / WASAPI 全部活在自己的线程内。
pub struct AudioPlayback {
    tx: Sender<AudioCommand>,
    thread: Option<JoinHandle<()>>,
    position: Arc<AtomicU64>,
    volume: Arc<AtomicU32>,
    paused: Arc<AtomicBool>,
    last_drift_check: Instant,
}

impl AudioPlayback {
    /// 打开音频流并默认暂停在 0。无音轨 / 打开失败只返回 None，视频继续播放。
    pub fn open(path: &Path, volume: u32) -> Option<Self> {
        let path = path.to_path_buf();
        let position = Arc::new(AtomicU64::new(0));
        let volume_state = Arc::new(AtomicU32::new(volume.min(100)));
        let paused = Arc::new(AtomicBool::new(true));
        let (tx, rx) = mpsc::channel();

        let thread_position = position.clone();
        let thread_volume = volume_state.clone();
        let thread_paused = paused.clone();
        let thread = thread::Builder::new()
            .name("wallpaper-audio".to_string())
            .spawn(move || {
                if let Err(err) = audio_loop(path, rx, &thread_position, &thread_volume) {
                    // 音频是侧向增强：失败只记日志，不回传 renderer-error。
                    log::warn!("[Audio] 音频链路停止: {:#}", err);
                }
                thread_paused.store(true, Ordering::Relaxed);
            })
            .ok()?;

        Some(Self {
            tx,
            thread: Some(thread),
            position,
            volume: volume_state,
            paused,
            last_drift_check: Instant::now(),
        })
    }

    pub fn set_volume(&mut self, volume: u32) {
        let volume = volume.min(100);
        if self.volume.swap(volume, Ordering::Relaxed) == volume {
            return;
        }
        let _ = self.tx.send(AudioCommand::SetVolume(volume));
    }

    /// fade / 全屏暂停时冻结音频；状态未变化时不发命令。
    pub fn set_paused(&mut self, paused: bool) {
        if self.paused.swap(paused, Ordering::Relaxed) == paused {
            return;
        }
        let _ = self.tx.send(if paused {
            AudioCommand::Pause
        } else {
            AudioCommand::Play
        });
    }

    /// crossfade 结束或循环回退时调用：音频与视频一起从 0 开始。
    pub fn reset(&mut self) {
        let _ = self.tx.send(AudioCommand::Seek(Duration::ZERO));
        if !self.paused.load(Ordering::Relaxed) {
            let _ = self.tx.send(AudioCommand::Play);
        }
    }

    /// 每帧调用，但只在 5s 周期用视频时钟校准一次音频位置。
    pub fn tick(&mut self, video_time: Duration, loop_duration: Duration, now: Instant) {
        if self.paused.load(Ordering::Relaxed) || loop_duration.is_zero() {
            return;
        }
        if now.duration_since(self.last_drift_check) < DRIFT_CHECK_INTERVAL {
            return;
        }
        self.last_drift_check = now;

        let target = Duration::from_nanos(
            (video_time.as_nanos() % loop_duration.as_nanos()).min(u64::MAX as u128) as u64,
        );
        let current = Duration::from_nanos(self.position.load(Ordering::Relaxed));
        let drift = if current > target {
            current.saturating_sub(target)
        } else {
            target.saturating_sub(current)
        };
        if drift > DRIFT_THRESHOLD {
            let _ = self.tx.send(AudioCommand::Seek(target));
        }
    }
}

impl Drop for AudioPlayback {
    fn drop(&mut self) {
        let _ = self.tx.send(AudioCommand::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct AudioComScope {
    initialized: bool,
}

impl AudioComScope {
    fn init() -> Self {
        unsafe {
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            if hr == RPC_E_CHANGED_MODE || hr.0 < 0 {
                return Self { initialized: false };
            }
            Self { initialized: true }
        }
    }
}

impl Drop for AudioComScope {
    fn drop(&mut self) {
        if self.initialized {
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        }
    }
}

fn audio_loop(
    path: PathBuf,
    rx: Receiver<AudioCommand>,
    position: &AtomicU64,
    volume: &AtomicU32,
) -> Result<()> {
    let _com = AudioComScope::init();

    let reader = open_audio_reader(&path)
        .with_context(|| format!("打开音频 SourceReader 失败 file={}", path.display()))?;
    log::info!("[Audio] 音频 SourceReader 已打开 file={}", path.display());
    let wasapi =
        WasapiOutput::open(volume.load(Ordering::Relaxed)).context("WASAPI 输出初始化失败")?;
    log::info!("[Audio] WASAPI 输出已初始化");
    configure_audio_format(&reader, &wasapi.format, wasapi.ieee_float)
        .context("音频 PCM 格式协商失败")?;
    log::info!("[Audio] 音频 PCM 格式协商完成");
    // 控制面 AudioPlayback.paused 初始为 true；audio 线程必须也以 paused 启动，
    // 否则新视频在 open() 后就已经出声，而后续 set_audio_paused(true) 因状态相同不会发 Pause。
    let mut ring: VecDeque<u8> = VecDeque::new();
    let mut paused = true;
    let mut pause_position = Duration::ZERO;

    loop {
        while let Ok(command) = rx.try_recv() {
            match command {
                AudioCommand::Pause => {
                    if !paused {
                        pause_position = Duration::from_nanos(position.load(Ordering::Relaxed));
                        unsafe {
                            wasapi.client.Stop()?;
                            wasapi.client.Reset()?;
                        }
                        ring.clear();
                        paused = true;
                    }
                }
                AudioCommand::Play => {
                    if paused {
                        seek_audio(&reader, pause_position, position, &mut ring)?;
                        unsafe { wasapi.client.Start()? };
                        paused = false;
                    }
                }
                AudioCommand::Seek(target) => {
                    seek_audio(&reader, target, position, &mut ring)?;
                    if paused {
                        pause_position = target;
                    }
                }
                AudioCommand::SetVolume(value) => wasapi.set_volume(value)?,
                AudioCommand::Shutdown => {
                    unsafe { wasapi.client.Stop()? };
                    return Ok(());
                }
            }
        }

        if paused {
            thread::sleep(Duration::from_millis(10));
            continue;
        }

        fill_render_buffer(&wasapi, &mut ring)?;
        if ring.len() < MAX_RING_BYTES {
            let sample_duration = pump_audio_sample(&reader, &mut ring, position)?;
            thread::sleep(sample_duration.min(Duration::from_millis(10)) / 2);
        } else {
            thread::sleep(Duration::from_millis(5));
        }
    }
}

fn open_audio_reader(path: &Path) -> Result<IMFSourceReader> {
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let url = windows::core::PCWSTR(wide.as_ptr());

    let attributes: IMFAttributes = unsafe {
        let mut value = None;
        MFCreateAttributes(&mut value, 0)?;
        value.context("MF 音频属性对象创建失败")?
    };
    let reader = unsafe { MFCreateSourceReaderFromURL(url, &attributes) }
        .with_context(|| format!("MF 无法打开音频文件 {:?}", path))?;

    unsafe {
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
        reader
            .SetStreamSelection(MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32, true)
            .context("视频没有可用音轨，静音降级")?;
    }
    Ok(reader)
}

/// 用 WASAPI 当前 mix format 反推 PCM 请求，避免采样率 / 声道数不一致导致初始化失败。
fn configure_audio_format(
    reader: &IMFSourceReader,
    format: &WAVEFORMATEX,
    ieee_float: bool,
) -> Result<()> {
    let output: IMFMediaType = unsafe { MFCreateMediaType() }?;
    let subtype = if ieee_float {
        MFAudioFormat_Float
    } else {
        MFAudioFormat_PCM
    };
    unsafe {
        output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
        output.SetGUID(&MF_MT_SUBTYPE, &subtype)?;
        output.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, format.nChannels as u32)?;
        output.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, format.nSamplesPerSec)?;
        output.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, format.wBitsPerSample as u32)?;
        output.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, format.nBlockAlign as u32)?;
        output.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, format.nAvgBytesPerSec)?;
        reader.SetCurrentMediaType(MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32, None, &output)?;
    }
    let w_format_tag = format.wFormatTag;
    let channels = format.nChannels;
    let sample_rate = format.nSamplesPerSec;
    let bits = format.wBitsPerSample;
    let block_align = format.nBlockAlign;
    log::info!(
        "[Audio] 输出格式协商完成 tag={} float={} channels={} rate={} bits={} block_align={}",
        w_format_tag,
        ieee_float,
        channels,
        sample_rate,
        bits,
        block_align
    );
    Ok(())
}

struct WasapiOutput {
    client: IAudioClient,
    render: IAudioRenderClient,
    volume: ISimpleAudioVolume,
    buffer_frames: usize,
    block_align: usize,
    format: WAVEFORMATEX,
    ieee_float: bool,
}

impl WasapiOutput {
    fn open(initial_volume: u32) -> Result<Self> {
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

            let mix = client.GetMixFormat()?;
            // GetMixFormat 可能返回 WAVEFORMATEXTENSIBLE。必须把原始完整结构保留到
            // Initialize 之后，不能只复制 WAVEFORMATEX 后立刻释放，否则 cbSize 扩展丢失，
            // 在 float / 多声道设备上会得到 E_INVALIDARG。
            let format = *mix;
            let w_format_tag = format.wFormatTag;
            let ieee_float = if w_format_tag == WAVE_FORMAT_IEEE_FLOAT {
                true
            } else if w_format_tag == WAVE_FORMAT_EXTENSIBLE {
                // WAVEFORMATEXTENSIBLE 是 packed(1)，SubFormat 从 WAVEFORMATEX(18 bytes)
                // 之后的第 6 个 u16 + u32 + GUID 偏移读取，不能取 packed struct 字段引用。
                let subformat_ptr = (mix as *const u8).add(18 + 2 + 4) as *const GUID;
                std::ptr::read_unaligned(subformat_ptr) == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
            } else {
                false
            };
            let init_result = client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                0,
                AUDIO_BUFFER_DURATION_HNS,
                0,
                mix,
                None,
            );
            CoTaskMemFree(Some(mix as *const c_void));
            init_result?;
            let buffer_frames = client.GetBufferSize()? as usize;
            let render: IAudioRenderClient = client.GetService()?;
            let volume: ISimpleAudioVolume = client.GetService()?;

            let output = Self {
                client,
                render,
                volume,
                buffer_frames,
                block_align: format.nBlockAlign as usize,
                format,
                ieee_float,
            };
            output.set_volume(initial_volume)?;
            Ok(output)
        }
    }

    fn set_volume(&self, value: u32) -> Result<()> {
        let level = value.min(100) as f32 / 100.0;
        unsafe {
            self.volume
                .SetMute(false, std::ptr::null())
                .context("取消 WASAPI 静音失败")?;
            self.volume
                .SetMasterVolume(level, std::ptr::null())
                .context("设置 WASAPI 音量失败")?;
        }
        Ok(())
    }
}

fn fill_render_buffer(output: &WasapiOutput, ring: &mut VecDeque<u8>) -> Result<()> {
    let padding = unsafe { output.client.GetCurrentPadding()? } as usize;
    if padding >= output.buffer_frames {
        return Ok(());
    }
    let available_bytes = (output.buffer_frames - padding) * output.block_align;
    let mut bytes = ring.len().min(available_bytes);
    bytes -= bytes % output.block_align;
    if bytes == 0 {
        return Ok(());
    }

    let frames = (bytes / output.block_align) as u32;
    let target = unsafe { output.render.GetBuffer(frames)? };
    let contiguous = ring.make_contiguous();
    unsafe { std::ptr::copy_nonoverlapping(contiguous.as_ptr(), target, bytes) };
    ring.drain(..bytes);
    unsafe { output.render.ReleaseBuffer(frames, 0)? };
    Ok(())
}

fn pump_audio_sample(
    reader: &IMFSourceReader,
    ring: &mut VecDeque<u8>,
    position: &AtomicU64,
) -> Result<Duration> {
    let mut flags = 0u32;
    let mut timestamp = 0i64;
    let mut sample: Option<IMFSample> = None;
    unsafe {
        reader.ReadSample(
            MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32,
            0,
            None,
            Some(&mut flags),
            Some(&mut timestamp),
            Some(&mut sample),
        )?;
    }

    if (flags as i32) & MF_SOURCE_READERF_ENDOFSTREAM.0 != 0 || sample.is_none() {
        seek_zero(reader, position, ring)?;
        return Ok(Duration::from_millis(10));
    }

    let sample = sample.context("SourceReader 返回空音频 sample")?;
    let duration = unsafe { sample.GetSampleDuration() }.unwrap_or(0).max(0) as u64;
    let buffer = unsafe { sample.ConvertToContiguousBuffer() }?;

    let mut ptr: *mut u8 = std::ptr::null_mut();
    let mut max_len = 0u32;
    let mut current_len = 0u32;
    unsafe { buffer.Lock(&mut ptr, Some(&mut max_len), Some(&mut current_len))? };
    let copied = (|| -> Result<()> {
        if ptr.is_null() {
            return Err(anyhow!("音频 sample 缓冲区指针为空"));
        }
        // IMFDXGIBuffer/IMFMediaBuffer::Lock 的 max_len 是缓冲区容量，current_len 才是本次
        // sample 的有效字节数。取 max 会把旧数据或空闲容量写进 WASAPI ring buffer。
        let len = current_len as usize;
        if len > 0 {
            ring.extend(
                unsafe { std::slice::from_raw_parts(ptr, len) }
                    .iter()
                    .copied(),
            );
        }
        Ok(())
    })();
    unsafe { buffer.Unlock()? };
    copied?;

    position.store(timestamp.max(0) as u64, Ordering::Relaxed);
    Ok(hns_to_duration(duration))
}

fn seek_audio(
    reader: &IMFSourceReader,
    target: Duration,
    position: &AtomicU64,
    ring: &mut VecDeque<u8>,
) -> Result<()> {
    unsafe {
        reader.Flush(MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32)?;
        reader.SetCurrentPosition(&GUID::default(), &seek_position(target))?;
    }
    ring.clear();
    position.store(duration_to_hns(target).max(0) as u64, Ordering::Relaxed);
    Ok(())
}

fn seek_zero(
    reader: &IMFSourceReader,
    position: &AtomicU64,
    ring: &mut VecDeque<u8>,
) -> Result<()> {
    seek_audio(reader, Duration::ZERO, position, ring)
}

fn seek_position(target: Duration) -> PROPVARIANT {
    PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_I8,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: PROPVARIANT_0_0_0 {
                    hVal: duration_to_hns(target),
                },
            }),
        },
    }
}

fn duration_to_hns(value: Duration) -> i64 {
    (value.as_nanos().min(i64::MAX as u128 / 100) / 100) as i64
}

fn hns_to_duration(value: u64) -> Duration {
    Duration::from_nanos((value as u128).saturating_mul(100).min(u64::MAX as u128) as u64)
}
