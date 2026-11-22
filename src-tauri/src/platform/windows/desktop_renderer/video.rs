//! DXVA 硬件解码视频链路：所有视频 session 由一个全局 worker 管理，renderer 只读本地纹理。
//!
//! 为什么单独 worker：MF 解码器、SourceReader、device manager 会在自己的线程占用 D3D11
//! device；renderer 直接消费 DXVA sample 会争抢同一个 device/context，表现为 device removed
//! 或线程冻结。这里让全局 worker 独占解码设备，多个视频以 session 形式共存，VideoProcessor
//! 输出到 keyed-mutex 共享纹理，renderer 只打开该纹理并短暂持有 mutex 复制本地纹理。

use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use windows::core::{Interface, BOOL, GUID};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_UNKNOWN, D3D_SRV_DIMENSION_TEXTURE2D};
use windows::Win32::Graphics::Direct3D10::ID3D10Multithread;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11InfoQueue,
    ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11VideoContext, ID3D11VideoDevice,
    ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator, ID3D11VideoProcessorInputView,
    ID3D11VideoProcessorOutputView, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_DEBUG, D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
    D3D11_MESSAGE, D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX, D3D11_SDK_VERSION,
    D3D11_SHADER_RESOURCE_VIEW_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC_0, D3D11_SUBRESOURCE_DATA,
    D3D11_TEX2D_SRV, D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
    D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_STREAM,
    D3D11_VIDEO_USAGE_PLAYBACK_NORMAL, D3D11_VPIV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_NV12, DXGI_FORMAT_R8G8_UNORM, DXGI_FORMAT_R8_UNORM, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{IDXGIAdapter, IDXGIDevice, IDXGIKeyedMutex, IDXGIResource};
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFDXGIBuffer, IMFDXGIDeviceManager, IMFMediaType, IMFSample, IMFSourceReader,
    MFCreateAttributes, MFCreateDXGIDeviceManager, MFCreateMediaType, MFCreateSourceReaderFromURL,
    MFMediaType_Video, MFShutdown, MFStartup, MFVideoFormat_NV12, MFSTARTUP_FULL, MF_MT_FRAME_SIZE,
    MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_MT_VIDEO_NOMINAL_RANGE,
    MF_MT_YUV_MATRIX, MF_PD_DURATION, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS,
    MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED, MF_SOURCE_READERF_ENDOFSTREAM,
    MF_SOURCE_READERF_ERROR, MF_SOURCE_READERF_NATIVEMEDIATYPECHANGED,
    MF_SOURCE_READER_ALL_STREAMS, MF_SOURCE_READER_D3D_MANAGER, MF_SOURCE_READER_DISABLE_DXVA,
    MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_SOURCE_READER_MEDIASOURCE, MF_VERSION,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToUInt64};

use super::audio::AudioPlayback;

pub fn startup() -> Result<()> {
    unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.context("Media Foundation 初始化失败")
}

/// 暂停原因按位合并：全屏暂停与 fade 暂停互不覆盖。
pub const PAUSE_FULLSCREEN: u8 = 1;
/// fade（交叉淡入淡出）期间暂停，可与全屏暂停同时存在。
pub const PAUSE_FADE: u8 = 2;

/// 只在 renderer 退出、所有 SourceReader / worker 都已经释放后调用一次。
/// MFStartup / MFShutdown 必须成对，否则 MF 平台 work queue / codec cache 会常驻到进程退出。
pub fn shutdown() -> Result<()> {
    unsafe { MFShutdown() }.context("Media Foundation 关闭失败")
}

/// 视频身份：同一路径 + 长度 + mtime 视为同一份视频，避免重复打开 worker。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoIdentity {
    path: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
}

impl VideoIdentity {
    pub fn from_path(path: &Path) -> Self {
        let metadata = std::fs::metadata(path).ok();
        Self {
            path: path.to_path_buf(),
            len: metadata.as_ref().map(|m| m.len()).unwrap_or(0),
            modified: metadata.and_then(|m| m.modified().ok()),
        }
    }
}

/// renderer 实际绘制的帧。视频帧始终来自 renderer 自己的本地纹理，
/// 不再让每块 surface 直接持有 shared keyed mutex。
pub struct VideoFrameSnapshot {
    pub view: ID3D11ShaderResourceView,
    pub uv_view: Option<ID3D11ShaderResourceView>,
    pub width: u32,
    pub height: u32,
}

impl VideoFrameSnapshot {
    pub fn for_static_frame(view: ID3D11ShaderResourceView, width: u32, height: u32) -> Self {
        Self {
            view,
            uv_view: None,
            width,
            height,
        }
    }
}

/// renderer 侧持有的视频帧资源：shared texture 只用于同步复制，实际绘制读本地 NV12。
struct VideoFrameSource {
    shared_texture: ID3D11Texture2D,
    // worker 与 renderer 用 keyed mutex 交替访问 shared texture：worker 持 key0 写入后置 key1，
    // renderer 持 key1 复制后置 key0（见 process_sample / begin_frame）。
    keyed_mutex: IDXGIKeyedMutex,
    // renderer 私有 NV12 副本，才是真正被采样绘制的纹理；shared texture 只作传输通道。
    local_texture: ID3D11Texture2D,
    local_y_view: ID3D11ShaderResourceView,
    local_uv_view: ID3D11ShaderResourceView,
    width: u32,
    height: u32,
}

/// worker 写、renderer 克隆读的跨线程快照。用 Mutex 只为安全传递，帧同步不靠它，
/// 真正的 mutex 在共享纹理的 keyed mutex 上（renderer 不持任何 worker 设备）。
#[derive(Clone, Default)]
struct SharedState {
    /// 当前帧在循环内的时间点，供 renderer 校准音频时钟。
    frame_time: Duration,
    /// 当前帧的真实媒体时长，决定该帧展示多久（pacing 用真实时长而非固定值）。
    frame_duration: Duration,
    /// 整段视频的循环时长；首圈会被最大帧结束时间修正，避免下圈全速追赶。
    loop_duration: Duration,
    /// 每次 reset 自增，renderer 可据此识别时间线已被重置。
    generation: u64,
    /// 首帧 Blt 成功才置 true；renderer 在此前不采样，避免全 0 NV12 解成绿屏。
    ready: bool,
}

/// worker 打开 session 后回传给 renderer 的共享纹理句柄与尺寸。
/// renderer 用 handle 在自己的设备上 OpenSharedResource 拿到 shared texture。
#[derive(Clone)]
struct SharedFrameInfo {
    /// GetSharedHandle 得到的 NT 句柄值，仅用于在 renderer 设备打开共享纹理。
    handle: isize,
    width: u32,
    height: u32,
}

/// renderer → 全局 worker 的控制命令。所有命令都在 worker 线程串行消费，
/// 避免多线程同时操作同一 D3D11 device / SourceReader。
enum WorkerCommand {
    /// 新建 session；ack 回传共享纹理信息，打开侧会等首帧 ready 后才返回给 renderer。
    OpenSession {
        id: u64,
        path: PathBuf,
        state: Arc<Mutex<SharedState>>,
        ack: Sender<Result<SharedFrameInfo>>,
    },
    /// 关闭并移除 session，由 VideoPlayback::drop 发出。
    CloseSession { id: u64 },
    /// 更新 worker 侧暂停时钟；renderer 先按原因合并，再发这一开/关命令。
    Pause { id: u64, paused: bool, now: Instant },
    /// 重置解码与时间线（seek 到 0、generation+1）；ack 确认 worker 已完成。
    Reset {
        id: u64,
        now: Instant,
        ack: Sender<Result<()>>,
    },
    /// worker 退出：清空所有 session 后结束主循环。
    Shutdown,
}

/// VideoWorkerHost 的内部状态：命令发送端、session id 分配器、worker 线程句柄。
/// Drop 时先发 Shutdown 再 join，保证 SourceReader / 设备按序释放。
struct VideoWorkerInner {
    commands: Mutex<Sender<WorkerCommand>>,
    /// 自增分配 session id，避免 id 复用导致旧命令误伤新 session。
    next_id: AtomicU64,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for VideoWorkerInner {
    fn drop(&mut self) {
        let _ = self.commands.lock().unwrap().send(WorkerCommand::Shutdown);
        if let Some(worker) = self.worker.lock().unwrap().take() {
            let _ = worker.join();
        }
    }
}

/// 全局视频 worker 的可克隆 handle。真正线程与 session map 都在 worker 内。
#[derive(Clone)]
pub struct VideoWorkerHost {
    inner: Arc<VideoWorkerInner>,
}

impl VideoWorkerHost {
    pub fn start(renderer_device: &ID3D11Device) -> Result<Self> {
        let (commands, receiver) = mpsc::channel();
        let worker_device = renderer_device.clone();
        let worker = thread::Builder::new()
            .name("wallpaper-video-worker".to_string())
            .spawn(move || {
                if let Err(err) = video_worker_main(&worker_device, receiver) {
                    log::error!("[VideoHw] worker 退出: {:#}", err);
                }
            })
            .context("全局 DXVA worker 线程创建失败")?;

        Ok(Self {
            inner: Arc::new(VideoWorkerInner {
                commands: Mutex::new(commands),
                next_id: AtomicU64::new(1),
                worker: Mutex::new(Some(worker)),
            }),
        })
    }

    fn next_session_id(&self) -> u64 {
        self.inner.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn send(&self, command: WorkerCommand) -> Result<()> {
        self.inner
            .commands
            .lock()
            .unwrap()
            .send(command)
            .context("全局 DXVA worker 命令通道关闭")
    }

    fn open_session(
        &self,
        path: &Path,
        state: Arc<Mutex<SharedState>>,
    ) -> Result<(u64, SharedFrameInfo)> {
        let id = self.next_session_id();
        let (ack_tx, ack_rx) = mpsc::channel();
        self.send(WorkerCommand::OpenSession {
            id,
            path: path.to_path_buf(),
            state,
            ack: ack_tx,
        })?;
        let info = ack_rx
            .recv_timeout(Duration::from_secs(10))
            .context("等待 DXVA session 打开超时")??;
        Ok((id, info))
    }
}

/// renderer 侧的轻量视频播放 handle；不再独占 worker / D3D11 device。
pub struct VideoPlayback {
    worker: VideoWorkerHost,
    /// 本视频对应的 worker session id；所有控制命令都带上它来定位 session。
    session_id: u64,
    /// worker 写入、renderer 克隆读的共享快照；不靠它做帧同步（同步在 keyed mutex）。
    state: Arc<Mutex<SharedState>>,
    context: ID3D11DeviceContext,
    frame: Option<VideoFrameSource>,
    /// 是否已有至少一帧从 shared texture 成功拷到本地纹理；未 ready 时不提供 snapshot。
    frame_ready: bool,
    audio: Option<AudioPlayback>,
    started_at: Instant,
    /// 暂停累计时长；计算墙钟 target 时扣除，暂停期间不追赶解码。
    paused_total: Duration,
    /// 暂停起点；Some 表示当前处于暂停累计状态，None 表示正在播放。
    paused_at: Option<Instant>,
    /// 按位记录的暂停原因（PAUSE_FULLSCREEN / PAUSE_FADE），全部清除才真正恢复 worker。
    pause_reasons: u8,
    frame_time: Duration,
    frame_duration: Duration,
    label: String,
    identity: VideoIdentity,
}

impl VideoPlayback {
    /// 打开一路视频：向 worker 申请 session，等首帧 ready 后建立本地纹理与旁挂音频。
    /// 任一步失败都会回收已创建的 session；无音轨只降级为静音，不影响视频播放。
    pub fn open(
        renderer_device: &ID3D11Device,
        renderer_context: &ID3D11DeviceContext,
        worker: &VideoWorkerHost,
        path: &Path,
        volume: u32,
    ) -> Result<Self> {
        let label = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("<unknown>")
            .to_string();
        let identity = VideoIdentity::from_path(path);
        let state = Arc::new(Mutex::new(SharedState::default()));
        let (session_id, info) = worker.open_session(path, state.clone())?;

        // 等 worker 首帧真正完成后再让 surface attach。这样 SetWallpaper 返回时视频已经
        // 可绘制，避免系统壁纸 → 空视频 surface → 首帧之间的黑屏窗口。
        let wait_deadline = Instant::now() + Duration::from_secs(10);
        while !state.lock().unwrap().ready {
            if Instant::now() >= wait_deadline {
                let _ = worker.send(WorkerCommand::CloseSession { id: session_id });
                return Err(anyhow::anyhow!("等待视频首帧超时 file={}", label));
            }
            thread::sleep(Duration::from_millis(2));
        }

        let frame = match open_shared_frame(renderer_device, &info) {
            Ok(frame) => frame,
            Err(err) => {
                let _ = worker.send(WorkerCommand::CloseSession { id: session_id });
                return Err(err);
            }
        };
        let audio = AudioPlayback::open(path, volume);

        log::info!(
            "[VideoHw] DXVA session 就绪 id={} file={} {}x{}",
            session_id,
            label,
            info.width,
            info.height
        );
        Ok(Self {
            worker: worker.clone(),
            session_id,
            state,
            context: renderer_context.clone(),
            frame: Some(frame),
            frame_ready: false,
            audio,
            started_at: Instant::now(),
            paused_total: Duration::ZERO,
            paused_at: None,
            pause_reasons: 0,
            frame_time: Duration::ZERO,
            frame_duration: Duration::from_millis(40),
            label,
            identity,
        })
    }

    pub fn identity(&self) -> &VideoIdentity {
        &self.identity
    }

    /// 每帧同步 worker 的 frame_time / frame_duration，并按视频时钟低频校准音频。
    /// 暂停时直接跳过，避免音频 seek 与暂停状态竞争。
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

    /// crossfade 结束后重置 worker 时间线与音频。注意 worker 的 reset 不清暂停原因，
    /// 若仍有 pause reason，这里立即补发 Pause，保持画面继续冻结（见下方 paused_at 处理）。
    pub fn reset_clock(&mut self, now: Instant) -> Result<()> {
        let (ack_tx, ack_rx) = mpsc::channel();
        self.worker.send(WorkerCommand::Reset {
            id: self.session_id,
            now,
            ack: ack_tx,
        })?;
        ack_rx
            .recv_timeout(Duration::from_secs(3))
            .context("等待 DXVA session 重置超时")??;
        self.started_at = now;
        self.paused_total = Duration::ZERO;
        // reset 会清空 worker 的暂停时长；若仍有 pause reason，reset 后立刻恢复暂停状态。
        self.paused_at = if self.pause_reasons == 0 {
            None
        } else {
            Some(now)
        };
        if self.pause_reasons != 0 {
            let _ = self.worker.send(WorkerCommand::Pause {
                id: self.session_id,
                paused: true,
                now,
            });
        }
        if let Some(audio) = self.audio.as_mut() {
            audio.reset();
        }
        Ok(())
    }

    /// 按原因暂停 / 恢复；只有所有原因都清除时才真正恢复 worker。
    pub fn set_pause_reason(&mut self, reason: u8, paused: bool, now: Instant) {
        let before = self.pause_reasons;
        if paused {
            self.pause_reasons |= reason;
        } else {
            self.pause_reasons &= !reason;
        }
        if before == 0 && self.pause_reasons != 0 {
            self.paused_at = Some(now);
            let _ = self.worker.send(WorkerCommand::Pause {
                id: self.session_id,
                paused: true,
                now,
            });
        } else if before != 0 && self.pause_reasons == 0 {
            if let Some(at) = self.paused_at.take() {
                self.paused_total = self
                    .paused_total
                    .saturating_add(now.saturating_duration_since(at));
            }
            let _ = self.worker.send(WorkerCommand::Pause {
                id: self.session_id,
                paused: false,
                now,
            });
        }
    }

    /// 音频暂停与视频暂停分开控制（例如某些场景只静音音频，画面继续播放）。
    pub fn set_audio_paused(&mut self, paused: bool) {
        if let Some(audio) = self.audio.as_mut() {
            audio.set_paused(paused);
        }
    }

    /// 音量 0..100，由音频线程应用到 WASAPI。
    pub fn set_audio_volume(&mut self, volume: u32) {
        if let Some(audio) = self.audio.as_mut() {
            audio.set_volume(volume);
        }
    }

    /// renderer 每帧开始时调用一次：尝试把 worker 产出的共享帧复制到本地纹理。
    /// 没有新帧时保留本地上一帧，所有 surface 继续显示同一画面，避免双屏闪烁。
    pub fn begin_frame(&mut self) {
        // worker 只有在 VideoProcessorBlt 成功并更新 state 后才 ready。首帧 ready 前
        // 即使 keyed mutex 处于可读状态也不绘制，避免把全 0 NV12 当有效帧采样成绿屏。
        if !self.state.lock().unwrap().ready {
            return;
        }
        let Some(frame) = self.frame.as_ref() else {
            return;
        };
        if unsafe { frame.keyed_mutex.AcquireSync(1, 0) }.is_err() {
            return;
        }
        unsafe {
            self.context
                .CopyResource(&frame.local_texture, &frame.shared_texture);
            self.context.Flush();
            let _ = frame.keyed_mutex.ReleaseSync(0);
        }
        self.frame_ready = true;
    }

    /// 是否已有至少一帧从共享纹理成功拷到本地纹理，可供绘制。
    pub fn is_frame_ready(&self) -> bool {
        self.frame_ready
    }

    /// 返回本地 NV12 的 Y/UV 两个 SRV 供 renderer 采样；尚未 ready 时返回 None。
    pub fn frame_snapshot(&self) -> Option<VideoFrameSnapshot> {
        if !self.frame_ready {
            return None;
        }
        let frame = self.frame.as_ref()?;
        Some(VideoFrameSnapshot {
            view: frame.local_y_view.clone(),
            uv_view: Some(frame.local_uv_view.clone()),
            width: frame.width,
            height: frame.height,
        })
    }
}

/// 销毁播放 handle 时只通知 worker 关闭对应 session；真正的设备 / 纹理释放由 worker 侧完成。
impl Drop for VideoPlayback {
    fn drop(&mut self) {
        log::info!(
            "[VideoHw] 释放 VideoPlayback session={} file={}",
            self.session_id,
            self.label
        );
        let _ = self.worker.send(WorkerCommand::CloseSession {
            id: self.session_id,
        });
    }
}

/// 在 renderer 设备上打开 worker 的共享纹理，并创建私有 NV12 副本及双平面 SRV。
/// 共享纹理不参与绘制，只作为跨线程搬运帧的通道；绘制始终读本地副本。
fn open_shared_frame(device: &ID3D11Device, info: &SharedFrameInfo) -> Result<VideoFrameSource> {
    let mut shared_texture: Option<ID3D11Texture2D> = None;
    let handle = HANDLE(info.handle as *mut c_void);
    unsafe { device.OpenSharedResource(handle, &mut shared_texture)? };
    let shared_texture = shared_texture.context("打开 DXVA 共享纹理失败")?;
    let keyed_mutex: IDXGIKeyedMutex = shared_texture.cast()?;

    // renderer 私有副本：NV12 单纹理双 SRV（Y=R8, UV=R8G8），显存比 BGRA8 副本少约 60%。
    let desc = D3D11_TEXTURE2D_DESC {
        Width: info.width,
        Height: info.height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_NV12,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    // 用视频黑初始化本地副本：Y=16, UV=128。即使 shader 在异常时序下提前采样，
    // 也只是黑色，不会把全 0 NV12 解成绿色。
    let initial = nv12_black_data(info.width, info.height);
    let initial_data = D3D11_SUBRESOURCE_DATA {
        pSysMem: initial.as_ptr() as *const c_void,
        SysMemPitch: info.width,
        SysMemSlicePitch: 0,
    };
    let mut local_texture: Option<ID3D11Texture2D> = None;
    unsafe { device.CreateTexture2D(&desc, Some(&initial_data), Some(&mut local_texture))? };
    let local_texture = local_texture.context("DXVA 本地副本纹理创建失败")?;

    let plane_view = |format| {
        let view_desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
            Format: format,
            ViewDimension: D3D_SRV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_SRV {
                    MostDetailedMip: 0,
                    MipLevels: 1,
                },
            },
        };
        let mut view: Option<ID3D11ShaderResourceView> = None;
        unsafe {
            device.CreateShaderResourceView(&local_texture, Some(&view_desc), Some(&mut view))?
        };
        view.context("DXVA NV12 平面 SRV 创建失败")
    };
    let local_y_view = plane_view(DXGI_FORMAT_R8_UNORM)?;
    let local_uv_view = plane_view(DXGI_FORMAT_R8G8_UNORM)?;

    Ok(VideoFrameSource {
        shared_texture,
        keyed_mutex,
        local_texture,
        local_y_view,
        local_uv_view,
        width: info.width,
        height: info.height,
    })
}

/// worker 独占的 D3D11 设备与 MF device manager。每个 VideoSession 各持一个实例，
/// 不与 renderer 共享 device/context，避免 DXVA sample 与绘制争抢同一设备。
struct WorkerGpu {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    video_device: ID3D11VideoDevice,
    /// 由 device manager 取出的视频上下文，用于 VideoProcessorBlt；取不到时回退到 worker context。
    video_context: ID3D11VideoContext,
    /// MF SourceReader 凭它拿到解码设备；worker 退出时必须 CloseDeviceHandle。
    manager: IMFDXGIDeviceManager,
    manager_handle: HANDLE,
    /// VP Blt 的目标 staging texture 按分辨率复用，避免每个 session 各持一份 NV12。
    staging_output: std::sync::Mutex<HashMap<(u32, u32), ID3D11Texture2D>>,
}

/// 关闭 device manager 的 device handle。调用方需保证 SourceReader / view / texture 先释放
/// （VideoSession 把 gpu 字段放在最后 drop），否则会先于引用它的解码资源销毁。
impl Drop for WorkerGpu {
    fn drop(&mut self) {
        unsafe {
            let _ = self.manager.CloseDeviceHandle(self.manager_handle);
        }
    }
}

impl WorkerGpu {
    /// 在与 renderer 相同的 adapter 上创建独立 D3D11 device，并注册到 MF device manager，
    /// 供 SourceReader 走 DXVA 硬解。debug layer 默认关闭：长运行下它会缓存已释放对象、
    /// 抬高内存占用，只有环境变量显式开启时才启用，且未安装时自动回退普通 device。
    fn new(renderer_device: &ID3D11Device) -> Result<Self> {
        let dxgi_device: IDXGIDevice = renderer_device.cast()?;
        let adapter: IDXGIAdapter = unsafe { dxgi_device.GetAdapter()? };
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        let base_flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT;
        // D3D11 debug layer 会为对象生命周期跟踪持有内部引用，长期运行的 dev 构建里
        // 会让“已释放资源”仍计入内存；默认关闭，需要时用环境变量显式打开。
        let debug_layer = std::env::var("MINI_WALLPAPER_D3D11_DEBUG")
            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let flags = if debug_layer {
            base_flags | D3D11_CREATE_DEVICE_DEBUG
        } else {
            base_flags
        };
        unsafe {
            match D3D11CreateDevice(
                Some(&adapter),
                D3D_DRIVER_TYPE_UNKNOWN,
                windows::Win32::Foundation::HMODULE::default(),
                flags,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            ) {
                Ok(()) => {
                    if debug_layer {
                        log::warn!("[VideoHw] D3D11 debug layer 已启用");
                    }
                }
                Err(err) => {
                    if !debug_layer {
                        return Err(err.into());
                    }
                    // Debug layer 未安装时不能阻断正常运行；回退到无 debug 的普通 device。
                    log::warn!(
                        "[VideoHw] D3D11 debug layer 不可用，回退普通 device: {:#}",
                        err
                    );
                    device = None;
                    context = None;
                    D3D11CreateDevice(
                        Some(&adapter),
                        D3D_DRIVER_TYPE_UNKNOWN,
                        windows::Win32::Foundation::HMODULE::default(),
                        base_flags,
                        None,
                        D3D11_SDK_VERSION,
                        Some(&mut device),
                        None,
                        Some(&mut context),
                    )?;
                }
            }
        }
        let device = device.context("DXVA worker device 创建失败")?;
        let context = context.context("DXVA worker context 创建失败")?;
        // SourceReader 可能在回调线程访问该 device，开启多线程保护以避开内部同步问题。
        if let Ok(multithread) = device.cast::<ID3D10Multithread>() {
            let _ = unsafe { multithread.SetMultithreadProtected(true) };
        } else {
            log::warn!("[VideoHw] ID3D10Multithread 不可用，跳过多线程保护");
        }

        let mut reset_token = 0u32;
        let mut manager: Option<IMFDXGIDeviceManager> = None;
        unsafe { MFCreateDXGIDeviceManager(&mut reset_token, &mut manager)? };
        let manager = manager.context("IMFDXGIDeviceManager 创建失败")?;
        unsafe { manager.ResetDevice(&device, reset_token)? };
        // SourceReader 凭此 handle 取得解码设备；与 reset_token 配套使用。
        let manager_handle = unsafe { manager.OpenDeviceHandle() }?;

        let video_device: ID3D11VideoDevice = device.cast()?;
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
        log::info!("[VideoHw] 全局 VideoContext 就绪");
        Ok(Self {
            device,
            context,
            video_device,
            video_context,
            manager,
            manager_handle,
            staging_output: Mutex::new(HashMap::new()),
        })
    }

    /// 按 (width, height) 复用 VideoProcessorBlt 的目标 staging 纹理。
    /// Blt 不直接写 shared keyed-mutex texture：那样会与 renderer 争抢 mutex、形成 OUTPUTHAZARD。
    /// 每帧先 Blt 到 worker 私有纹理，再 CopyResource 到 shared texture。
    fn staging_output_texture(&self, width: u32, height: u32) -> Result<ID3D11Texture2D> {
        let key = (width, height);
        if let Some(texture) = self.staging_output.lock().unwrap().get(&key) {
            return Ok(texture.clone());
        }
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_NV12,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            CPUAccessFlags: 0,
            // 纯 worker 私有纹理：无需 shared / keyed-mutex 标志，避免与 renderer 产生同步耦合。
            MiscFlags: 0,
        };
        let mut texture: Option<ID3D11Texture2D> = None;
        unsafe {
            self.device
                .CreateTexture2D(&desc, None, Some(&mut texture))?
        };
        let texture = texture.context("VideoProcessor staging texture 创建失败")?;
        self.staging_output
            .lock()
            .unwrap()
            .insert(key, texture.clone());
        Ok(texture)
    }
}

/// 只在失败时读取 D3D11 debug layer 的 info queue，不改变设备状态。
fn log_d3d11_info_queue(device: &ID3D11Device, stage: &str) {
    let Ok(queue) = device.cast::<ID3D11InfoQueue>() else {
        return;
    };
    unsafe {
        let count = queue.GetNumStoredMessages();
        if count == 0 {
            log::warn!("[VideoHw] D3D11 info queue {} 为空", stage);
            return;
        }
        let start = count.saturating_sub(8);
        for index in start..count {
            let mut byte_length = 0usize;
            if queue.GetMessage(index, None, &mut byte_length).is_err() || byte_length == 0 {
                continue;
            }

            // GetMessage 写入的是 D3D11_MESSAGE + 变长 description，必须传入
            // 一整段足够大的对齐缓冲区；直接传栈上 D3D11_MESSAGE 会造成越界写。
            let mut words = vec![0usize; byte_length.div_ceil(std::mem::size_of::<usize>())];
            let message_ptr = words.as_mut_ptr() as *mut D3D11_MESSAGE;
            if queue
                .GetMessage(index, Some(message_ptr), &mut byte_length)
                .is_err()
            {
                continue;
            }
            let message = &*message_ptr;
            let description =
                if message.pDescription.is_null() || message.DescriptionByteLength == 0 {
                    String::new()
                } else {
                    let len = message.DescriptionByteLength.min(byte_length);
                    String::from_utf8_lossy(std::slice::from_raw_parts(message.pDescription, len))
                        .trim_end_matches('\0')
                        .to_string()
                };
            log::warn!(
                "[VideoHw] D3D11 info queue {} #{}/{} category={:?} severity={:?} id={:?} {}",
                stage,
                index + 1,
                count,
                message.Category,
                message.Severity,
                message.ID,
                description
            );
        }
        queue.ClearStoredMessages();
    }
}

/// 一帧解码后的 DXVA sample 及其媒体时间信息。
/// timestamp / duration 统一从 100ns 换算成 Duration，并对负值 / 0 做兜底。
struct DecodedSample {
    sample: IMFSample,
    timestamp: Duration,
    duration: Duration,
    /// MF 原始 flags，仅用于诊断（媒体类型变化 / 错误标志）。
    flags: u32,
}

impl DecodedSample {
    fn new(sample: IMFSample, timestamp: i64, duration: i64, flags: u32) -> Self {
        Self {
            sample,
            timestamp: hns_to_duration(timestamp.max(0) as u64),
            duration: normalize_duration(hns_to_duration(duration.max(0) as u64)),
            flags,
        }
    }
}

/// 一路视频的解码会话：独占 SourceReader / VideoProcessor / GPU 资源，由全局 worker 串行 tick。
/// 字段 drop 顺序有意义：gpu 必须最后释放（见 WorkerGpu::drop）。
struct VideoSession {
    id: u64,
    /// 与 renderer 共享的只读快照，首帧 / 每帧更新其中的时间与 ready。
    state: Arc<Mutex<SharedState>>,
    reader: IMFSourceReader,
    enumerator: Option<ID3D11VideoProcessorEnumerator>,
    processor: Option<ID3D11VideoProcessor>,
    shared_texture: Option<ID3D11Texture2D>,
    shared_info: Option<SharedFrameInfo>,
    output_view: Option<ID3D11VideoProcessorOutputView>,
    width: u32,
    height: u32,
    /// 实际用于回绕的循环时长；首圈会被最大帧结束时间修正。
    loop_duration: Duration,
    /// 从 MF_PD_DURATION 读到的容器时长，作为 loop_duration 的初始 / reset 基准。
    presentation_duration: Duration,
    started: Instant,
    /// 暂停累计时长，计算播放目标时扣除，暂停期间不追赶解码。
    paused_total: Duration,
    /// worker 侧暂停起点；Some 表示暂停中，恢复时累加到 paused_total。
    paused_at: Option<Instant>,
    /// 已播放循环的时间基准；每圈 EOF 后累加 loop_duration，避免时间戳回绕。
    cycle_base: Duration,
    /// Blt 失败时暂存当前 sample，下一 tick 优先重试而不是丢帧。
    pending: Option<DecodedSample>,
    /// 以下四个是“只记一次”的诊断开关，避免高频日志刷屏。
    first_frame_logged: bool,
    stage_logged: bool,
    blt_error_logged: bool,
    decoder_desc_logged: bool,
    // 必须最后 drop：先释放 SourceReader / view / texture，再关闭 device manager。
    gpu: WorkerGpu,
}

impl VideoSession {
    /// 创建一路解码 session：配置 SourceReader 输出 NV12、读取媒体时长、建立共享纹理与
    /// VideoProcessor。每个 session 独立持有一个 WorkerGpu，不复用其它 session 的设备。
    fn new(
        renderer_device: &ID3D11Device,
        id: u64,
        path: &Path,
        state: Arc<Mutex<SharedState>>,
    ) -> Result<Self> {
        let gpu = WorkerGpu::new(renderer_device)?;
        let attributes: IMFAttributes = unsafe {
            let mut value = None;
            MFCreateAttributes(&mut value, 3)?;
            value.context("MF attributes 创建失败")?
        };
        unsafe {
            attributes.SetUnknown(&MF_SOURCE_READER_D3D_MANAGER, &gpu.manager)?;
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
        // 先关闭所有流再只启用第一路视频，省掉音频 / 字幕流的解码与同步开销。
        unsafe {
            reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
            reader.SetStreamSelection(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, true)?;
        }
        // 请求 NV12 输出，与共享纹理格式及 renderer 的双平面采样保持一致。
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

        let media_type =
            unsafe { reader.GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)? };
        let frame_size = unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE)? };
        let width = (frame_size >> 32) as u32;
        let height = frame_size as u32;
        if width == 0 || height == 0 {
            return Err(anyhow::anyhow!("视频尺寸无效 {}x{}", width, height));
        }

        let presentation_duration = read_presentation_duration(&reader).unwrap_or(Duration::ZERO);
        if presentation_duration.is_zero() {
            log::warn!(
                "[VideoHw] session={} MF_PD_DURATION 不可用，改用最大帧时间推断循环时长",
                id
            );
        } else {
            log::info!(
                "[VideoHw] session={} 视频时长 {:.3}s",
                id,
                presentation_duration.as_secs_f64()
            );
        }

        let mut session = Self {
            id,
            state,
            gpu,
            reader,
            enumerator: None,
            processor: None,
            shared_texture: None,
            shared_info: None,
            output_view: None,
            width: 0,
            height: 0,
            loop_duration: presentation_duration,
            presentation_duration,
            started: Instant::now(),
            paused_total: Duration::ZERO,
            paused_at: None,
            cycle_base: Duration::ZERO,
            pending: None,
            first_frame_logged: false,
            stage_logged: false,
            blt_error_logged: false,
            decoder_desc_logged: false,
        };
        session.ensure_output(width, height)?;
        Ok(session)
    }

    /// 返回共享纹理信息；仅在 new() 内 ensure_output 成功后才可调用。
    fn shared_frame_info(&self) -> &SharedFrameInfo {
        self.shared_info.as_ref().expect("共享纹理信息已就绪")
    }

    /// 建立（或尺寸变化时重建）NV12 共享纹理与 VideoProcessor 输出视图。
    /// 纹理带 KEYEDMUTEX 标志供 renderer 安全复制，并用视频黑初始化防止首帧前的绿屏。
    fn ensure_output(&mut self, width: u32, height: u32) -> Result<()> {
        if self.width == width && self.height == height && self.output_view.is_some() {
            return Ok(());
        }
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_NV12,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX.0 as u32,
        };
        // shared texture 也用视频黑初始化；worker 首帧前即使被打开，也不会是全 0 绿屏。
        let initial = nv12_black_data(width, height);
        let initial_data = D3D11_SUBRESOURCE_DATA {
            pSysMem: initial.as_ptr() as *const c_void,
            SysMemPitch: width,
            SysMemSlicePitch: 0,
        };
        let mut texture: Option<ID3D11Texture2D> = None;
        unsafe {
            self.gpu
                .device
                .CreateTexture2D(&desc, Some(&initial_data), Some(&mut texture))?
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
        let enumerator = unsafe {
            self.gpu
                .video_device
                .CreateVideoProcessorEnumerator(&content)
        }?;
        let format_support = unsafe { enumerator.CheckVideoProcessorFormat(DXGI_FORMAT_NV12)? };
        if format_support & D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT.0 as u32 == 0 {
            return Err(anyhow::anyhow!(
                "DXVA VideoProcessor 不支持 NV12 输出: support={}",
                format_support
            ));
        }
        let processor = unsafe { self.gpu.video_device.CreateVideoProcessor(&enumerator, 0) }?;

        // VideoProcessorBlt 不直接写 shared keyed-mutex texture；写 worker staging texture，
        // 成功后 CopyResource 到 shared texture。staging texture 按分辨率复用。
        let output_texture = self.gpu.staging_output_texture(width, height)?;

        let view_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
            },
        };
        let mut output_view: Option<ID3D11VideoProcessorOutputView> = None;
        unsafe {
            self.gpu.video_device.CreateVideoProcessorOutputView(
                &output_texture,
                &enumerator,
                &view_desc,
                Some(&mut output_view),
            )?;
        }
        let output_view = output_view.context("VideoProcessor output view 创建失败")?;

        let keyed_mutex: IDXGIKeyedMutex = texture.cast()?;
        // 把 keyed mutex 初始化为 key0 归属，renderer 才能用 key1 取得读锁。
        unsafe { keyed_mutex.AcquireSync(0, 0)? };
        let _ = unsafe { keyed_mutex.ReleaseSync(0) };

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
            "[VideoHw] session={} 共享纹理就绪 {}x{}",
            self.id,
            width,
            height
        );
        Ok(())
    }

    /// 从 SourceReader 拉取一帧解码 sample，记录 MF 原始 flags / 时间戳 / 时长。
    /// EOF 返回 None，由 tick 决定回绕；错误标志保留在 flags 里用于诊断。
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
        Ok(Some(DecodedSample::new(sample, timestamp, duration, flags)))
    }

    /// 只读诊断：打印当前 media type 的 interlace / matrix / range / frame_size，
    /// 用于定位 Blt 的 E_INVALIDARG，不改变任何设备状态。
    fn media_type_diag(&self) -> String {
        let Ok(media_type) = (unsafe {
            self.reader
                .GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)
        }) else {
            return "media_type=unavailable".to_string();
        };
        let interlace = unsafe { media_type.GetUINT32(&MF_MT_INTERLACE_MODE) }.unwrap_or(u32::MAX);
        let matrix = unsafe { media_type.GetUINT32(&MF_MT_YUV_MATRIX) }.unwrap_or(u32::MAX);
        let nominal =
            unsafe { media_type.GetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE) }.unwrap_or(u32::MAX);
        let frame_size = unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE) }.unwrap_or(u64::MAX);
        format!(
            "media_type interlace={} matrix={} nominal={} frame_size=0x{:x}",
            interlace, matrix, nominal, frame_size
        )
    }

    /// 把一帧 DXVA sample 经 VideoProcessorBlt 输出到 staging 纹理，再拷入 shared texture。
    /// 成功释放 key1（renderer 可 AcquireSync(1) 复制）；失败释放 key0 并保留 sample 供下轮重试。
    /// 返回是否真正产出新帧。
    fn process_sample(&mut self, decoded: &DecodedSample) -> Result<bool> {
        let output_view = self.output_view.clone().context("VideoProcessor 未就绪")?;
        let output_texture = self.gpu.staging_output_texture(self.width, self.height)?;
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
        // 解码器持有多帧纹理池，subresource index 定位当前帧在池中的数组切片。
        let subresource = unsafe { dxgi_buffer.GetSubresourceIndex() }?;
        // 只读诊断：区分 E_INVALIDARG 来自 input view 还是 Blt；不改变任何处理流程。
        let mut decoder_desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { decoder_texture.GetDesc(&mut decoder_desc) };
        if !self.decoder_desc_logged {
            self.decoder_desc_logged = true;
            log::info!(
                "[VideoHw] session={} decoder texture {}x{} fmt={:?} bind={} array={} mip={} subresource={}",
                self.id,
                decoder_desc.Width,
                decoder_desc.Height,
                decoder_desc.Format,
                decoder_desc.BindFlags,
                decoder_desc.ArraySize,
                decoder_desc.MipLevels,
                subresource
            );
        }

        let _ = unsafe { self.gpu.manager.TestDevice(self.gpu.manager_handle) };

        let shared_texture = self.shared_texture.clone().context("共享纹理缺失")?;
        let keyed_mutex: IDXGIKeyedMutex = shared_texture.cast()?;
        if unsafe { keyed_mutex.AcquireSync(0, 0) }.is_err() {
            return Ok(false);
        }
        if !self.stage_logged {
            self.stage_logged = true;
            log::info!("[VideoHw] session={} 共享纹理 key0 已获取", self.id);
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
                self.gpu
                    .video_device
                    .CreateVideoProcessorInputView(
                        &decoder_texture,
                        &enumerator,
                        &input_desc,
                        Some(&mut input_view),
                    )
                    .with_context(|| {
                        format!(
                            "CreateVideoProcessorInputView 失败 subresource={} out={}x{} decoder={}x{} fmt={:?} bind={} array={} mip={} flags=0x{:x} type_changed={} native_changed={} error_flag={} {}",
                            subresource,
                            self.width,
                            self.height,
                            decoder_desc.Width,
                            decoder_desc.Height,
                            decoder_desc.Format,
                            decoder_desc.BindFlags,
                            decoder_desc.ArraySize,
                            decoder_desc.MipLevels,
                            decoded.flags,
                            (decoded.flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32) != 0,
                            (decoded.flags & MF_SOURCE_READERF_NATIVEMEDIATYPECHANGED.0 as u32) != 0,
                            (decoded.flags & MF_SOURCE_READERF_ERROR.0 as u32) != 0,
                            self.media_type_diag()
                        )
                    })?;
            }
            let input_view = input_view.context("VideoProcessor input view 创建失败")?;
            let mut stream = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: BOOL(1),
                pInputSurface: ManuallyDrop::new(Some(input_view)),
                ..Default::default()
            };
            if !self.first_frame_logged {
                log::info!("[VideoHw] session={} 开始首帧 VideoProcessorBlt", self.id);
            }
            let blt_result = unsafe {
                self.gpu.video_context.VideoProcessorBlt(
                    &processor,
                    &output_view,
                    0,
                    std::slice::from_ref(&stream),
                )
            };
            unsafe { ManuallyDrop::drop(&mut stream.pInputSurface) };
            blt_result.with_context(|| {
                format!(
                    "VideoProcessorBlt 失败 subresource={} out={}x{} decoder={}x{} fmt={:?} bind={} array={} mip={} flags=0x{:x} type_changed={} native_changed={} error_flag={} {}",
                    subresource,
                    self.width,
                    self.height,
                    decoder_desc.Width,
                    decoder_desc.Height,
                    decoder_desc.Format,
                    decoder_desc.BindFlags,
                    decoder_desc.ArraySize,
                    decoder_desc.MipLevels,
                    decoded.flags,
                    (decoded.flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32) != 0,
                    (decoded.flags & MF_SOURCE_READERF_NATIVEMEDIATYPECHANGED.0 as u32) != 0,
                    (decoded.flags & MF_SOURCE_READERF_ERROR.0 as u32) != 0,
                    self.media_type_diag()
                )
            })?;
            unsafe {
                // Blt 只写 worker 私有纹理；确认 GPU 完成写入后再复制到 shared texture。
                self.gpu.context.Flush();
                self.gpu
                    .context
                    .CopyResource(&shared_texture, &output_texture);
                self.gpu.context.Flush();
            }
            Ok(())
        })();
        // key1 = 写入成功，renderer 可持 key1 复制；key0 = 失败 / 未就绪，仍由 worker 持有。
        let release_key = if result.is_ok() { 1 } else { 0 };
        let _ = unsafe { keyed_mutex.ReleaseSync(release_key) };
        match result {
            Ok(()) => {
                if !self.first_frame_logged {
                    self.first_frame_logged = true;
                    log::info!("[VideoHw] session={} 首帧 VideoProcessorBlt 完成", self.id);
                }
                Ok(true)
            }
            Err(err) => {
                if !self.blt_error_logged {
                    self.blt_error_logged = true;
                    log::warn!(
                        "[VideoHw] session={} VideoProcessorBlt 失败，保留 sample 重试: {:#}",
                        self.id,
                        err
                    );
                    log_d3d11_info_queue(&self.gpu.device, "VideoProcessorBlt");
                }
                Ok(false)
            }
        }
    }

    /// 首圈用“当前帧结束时间”修正 loop_duration。若按首帧 ~33ms 推断循环长度，
    /// 下次 EOF 只推进几十毫秒，墙钟 target 会远超 frame_time，导致全速追赶。
    fn note_frame(&mut self, frame_time: Duration, duration: Duration, first_cycle: bool) {
        if first_cycle {
            // 第一圈持续用最大帧结束时间修正 loop_duration；不能用第一帧的 33ms，
            // 否则下一次 EOF 只推进几十毫秒，墙钟 target 远大于 frame_time，全速追赶。
            self.loop_duration = self.loop_duration.max(frame_time.saturating_add(duration));
        }
    }

    /// Flush 解码器并返回起点，实现循环回退；PROPVARIANT 为 VT_I8 的 0。
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

    /// 重置时间线到 0 并让 generation 自增：renderer 借此识别新周期并重置音频。
    /// 本方法不清 pause_reasons；若仍暂停，由 renderer 的 reset_clock 负责补发 Pause。
    fn reset(&mut self, now: Instant) -> Result<()> {
        self.loop_duration = self.presentation_duration;
        self.seek_zero()?;
        self.started = now;
        self.paused_total = Duration::ZERO;
        self.paused_at = None;
        self.cycle_base = Duration::ZERO;
        self.pending = None;
        let mut shared = self.state.lock().unwrap();
        shared.frame_time = Duration::ZERO;
        shared.frame_duration = Duration::from_millis(40);
        shared.loop_duration = self.loop_duration;
        // generation 自增：renderer 可据此丢弃旧周期残留的 frame_time。
        shared.generation = shared.generation.wrapping_add(1);
        shared.ready = false;
        Ok(())
    }

    /// 记录 worker 侧暂停起点 / 累计时长；paused_at 为 Some 表示暂停中。
    /// renderer 已按原因合并，故这里只需处理一次开 / 关切换（重复命令被 match 的 _ 吞掉）。
    fn pause(&mut self, paused: bool, now: Instant) {
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

    /// worker 每次调度推进一帧：按真实媒体时长 pacing，必要时读取 / 重试 sample。
    /// 返回本轮是否产出帧或发生回绕，供主循环决定是否少睡一会。
    fn tick(&mut self) -> Result<bool> {
        if self.paused_at.is_some() {
            return Ok(false);
        }
        // 墙钟扣除暂停时长才是媒体时间；未到下一帧时间就跳过，避免解码快于展示。
        let target = self.started.elapsed().saturating_sub(self.paused_total);
        {
            let shared = self.state.lock().unwrap();
            if shared.ready && target < shared.frame_time.saturating_add(shared.frame_duration) {
                return Ok(false);
            }
        }

        // 上一轮 Blt 失败的 sample 优先重试，而不是继续 ReadSample 丢帧。
        let sample = match self.pending.take() {
            Some(sample) => sample,
            None => match self.read_sample()? {
                Some(sample) => sample,
                None => {
                    // EOF：用真实媒体时长作为一圈长度（首圈未知则暂用 target），
                    // 累加到 cycle_base 后回绕，避免循环播放时时间戳归零引发跳变。
                    let completed = if self.loop_duration.is_zero() {
                        target
                    } else {
                        self.loop_duration
                    };
                    self.loop_duration = completed;
                    self.cycle_base = self.cycle_base.saturating_add(completed);
                    {
                        let mut shared = self.state.lock().unwrap();
                        shared.frame_time = self.cycle_base;
                        shared.frame_duration = Duration::from_millis(40);
                        shared.loop_duration = self.loop_duration;
                    }
                    self.seek_zero()?;
                    return Ok(true);
                }
            },
        };

        match self.process_sample(&sample) {
            Ok(true) => {
                let frame_time = self.cycle_base.saturating_add(sample.timestamp);
                self.note_frame(frame_time, sample.duration, self.cycle_base.is_zero());
                let mut shared = self.state.lock().unwrap();
                shared.frame_time = frame_time;
                shared.frame_duration = sample.duration;
                shared.loop_duration = self.loop_duration;
                shared.ready = true;
                Ok(true)
            }
            Ok(false) => {
                self.pending = Some(sample);
                Ok(false)
            }
            Err(err) => Err(err),
        }
    }
}

/// 全局视频 worker 主循环：单线程串行处理所有 session 的命令与逐帧推进。
/// 每轮先排空命令队列，再遍历各 session tick；无工作时短暂 sleep 让出 CPU。
fn video_worker_main(
    renderer_device: &ID3D11Device,
    commands: Receiver<WorkerCommand>,
) -> Result<()> {
    let mut sessions: HashMap<u64, VideoSession> = HashMap::new();

    loop {
        // 先处理所有控制命令，保证 pause / reset / open / close 能及时生效。
        while let Ok(command) = commands.try_recv() {
            match command {
                WorkerCommand::OpenSession {
                    id,
                    path,
                    state,
                    ack,
                } => {
                    let result = (|| -> Result<SharedFrameInfo> {
                        let session = VideoSession::new(renderer_device, id, &path, state)?;
                        let info = session.shared_frame_info().clone();
                        sessions.insert(id, session);
                        log::info!(
                            "[VideoHw] session={} 已打开 active_sessions={} file={}",
                            id,
                            sessions.len(),
                            path.file_name()
                                .and_then(|v| v.to_str())
                                .unwrap_or("<unknown>")
                        );
                        Ok(info)
                    })();
                    let _ = ack.send(result);
                }
                WorkerCommand::CloseSession { id } => {
                    if sessions.remove(&id).is_some() {
                        log::info!(
                            "[VideoHw] session={} 已关闭 active_sessions={}",
                            id,
                            sessions.len()
                        );
                    }
                }
                WorkerCommand::Pause { id, paused, now } => {
                    if let Some(session) = sessions.get_mut(&id) {
                        session.pause(paused, now);
                    }
                }
                WorkerCommand::Reset { id, now, ack } => {
                    let result = if let Some(session) = sessions.get_mut(&id) {
                        session.reset(now)
                    } else {
                        Err(anyhow::anyhow!("DXVA session {} 不存在", id))
                    };
                    let _ = ack.send(result);
                }
                WorkerCommand::Shutdown => {
                    sessions.clear();
                    return Ok(());
                }
            }
        }

        if sessions.is_empty() {
            thread::sleep(Duration::from_millis(2));
            continue;
        }

        // 单线程串行 tick 所有 session：各 session 虽各有独立设备，但共用同一 worker 线程，
        // 避免跨线程同时操作 MF / D3D11 引发 device removed 或死锁。
        let ids: Vec<u64> = sessions.keys().copied().collect();
        let mut progressed = false;
        for id in ids {
            let Some(session) = sessions.get_mut(&id) else {
                continue;
            };
            match session.tick() {
                Ok(done) => progressed |= done,
                Err(err) => {
                    log::warn!("[VideoHw] session={} 帧处理失败: {:#}", id, err);
                    sessions.remove(&id);
                }
            }
        }
        // 本轮有帧产出或回绕就不睡，尽量追上墙钟节奏；否则稍睡避免忙等。
        if !progressed {
            thread::sleep(Duration::from_millis(1));
        }
    }
}

/// 构造 VT_I8 且值为 0 的 PROPVARIANT，用于把 SourceReader 定位到起点。
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

/// 读取 MF_PD_DURATION（100ns）；拿不到返回 None，让 session 用最大帧时间推断循环时长。
fn read_presentation_duration(reader: &IMFSourceReader) -> Option<Duration> {
    let mut variant = unsafe {
        reader.GetPresentationAttribute(
            MF_SOURCE_READER_MEDIASOURCE.0 as u32,
            &MF_PD_DURATION as *const GUID,
        )
    }
    .ok()?;
    let hns = unsafe { PropVariantToUInt64(&variant) }.ok();
    unsafe { PropVariantClear(&mut variant) }.ok()?;
    hns.filter(|value| *value > 0).map(hns_to_duration)
}

/// 100ns（MF 时间单位）转 Duration，饱和乘法防溢出。
fn hns_to_duration(hns: u64) -> Duration {
    Duration::from_nanos((hns as u128).saturating_mul(100).min(u64::MAX as u128) as u64)
}

/// sample 时长为 0 时回退到 40ms，保证 pacing 始终有最小帧间隔。
fn normalize_duration(duration: Duration) -> Duration {
    if duration.is_zero() {
        Duration::from_millis(40)
    } else {
        duration
    }
}

/// NV12 视频黑：Y=16，UV=128。全 0 会被 BT.601 shader 解成绿色。
fn nv12_black_data(width: u32, height: u32) -> Vec<u8> {
    let y_plane = width as usize * height as usize;
    let uv_height = (height as usize + 1) / 2;
    let uv_plane = width as usize * uv_height;
    let mut data = vec![0u8; y_plane + uv_plane];
    data[..y_plane].fill(16);
    data[y_plane..].fill(128);
    data
}
