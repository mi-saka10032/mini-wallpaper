use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::image;

use anyhow::{anyhow, Context, Result};
use tokio::sync::oneshot;
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_QUIT,
};

use super::surface::{RenderSurface, SharedDevice, SharedFrames, WallpaperMedia};
use super::video::{
    shutdown as mf_shutdown, startup as mf_startup, VideoIdentity, VideoPlayback, PAUSE_FADE,
    PAUSE_FULLSCREEN,
};
use crate::events::{RendererErrorPayload, TypedEmit};
use crate::platform::wallpaper_renderer::frame_cache::{
    cache_key_with_target, CacheKey, FrameCache,
};
use crate::platform::wallpaper_renderer::{
    ExtendViewport, FitMode, MonitorRect, RendererErrorCode, WallpaperKind,
};
use tauri::Manager;

/// 在途的一次性图片解码：worker 线程完成后，结果经 `decode_tx` 回到渲染线程 attach。
/// 只活到结果回收；过期 generation 的结果被丢弃，ack 仍回 Ok 避免误报 error。
struct PendingDecode {
    key: Option<CacheKey>,
    monitor_ids: Vec<String>,
    fit_mode: FitMode,
    crossfade: bool,
    // 同 key 的重复命令会合并到同一次解码，所有等待方都要收到回执。
    acks: Vec<Ack>,
    handle: JoinHandle<()>,
}

/// 给所有等待方回同一个结果（Ack 是 oneshot，逐个 send）。
fn send_acks(acks: Vec<Ack>, result: Result<(), String>) {
    for ack in acks {
        send_ack(Some(ack), result.clone());
    }
}

/// worker → 渲染线程的解码结果。`generation` 用于在连点时丢弃过期结果。
struct DecodeResult {
    generation: u64,
    result: Result<SharedFrames, String>,
}

/// 确保共享 device 存在：设备丢失时被 take() 后，下一次正常命令在此惰性重建。
/// 重建会连带新建 video worker（它持有 device clone，旧的那份已随设备失效）。
fn ensure_shared(shared: &mut Option<SharedDevice>) -> Result<&SharedDevice> {
    if shared.is_none() {
        log::info!("[Renderer] 设备丢失后重建共享 D3D11 上下文");
        *shared = Some(SharedDevice::new().context("重建共享 D3D11 上下文失败")?);
    }
    Ok(shared.as_ref().expect("ensure_shared 刚初始化过"))
}

/// 把媒体挂到组内每个 surface，并回 ack。命中缓存的同步路径与 worker 回来的
/// 异步路径共用这一份逻辑，保证「组级共用同一个 now」的行为不变。
fn attach_media(
    surfaces: &mut HashMap<String, RenderSurface>,
    monitor_ids: &[String],
    media: WallpaperMedia,
    fit_mode: FitMode,
    crossfade: bool,
    now: Instant,
    ack: Option<Ack>,
) {
    let mut attached = false;
    for monitor_id in monitor_ids {
        if let Some(surface) = surfaces.get_mut(monitor_id) {
            surface.queue_wallpaper(media.clone(), fit_mode, crossfade, now);
            attached = true;
        } else {
            log::warn!("[Renderer] {} 组换图时原生表面不存在", monitor_id);
        }
    }
    if attached {
        send_ack(ack, Ok(()));
    } else {
        let message = format!("组换图时所有原生表面都不存在: {:?}", monitor_ids);
        log::warn!("[Renderer] {}", message);
        send_ack(ack, Err(message));
    }
}

/// 关键命令的异步回执：把 renderer 线程内的解码 / surface 失败回传给协调器。
/// 只用于 EnsureMonitor / SetWallpaper——这两条会真正做 D3D11 资源创建与解码。
type Ack = oneshot::Sender<Result<(), String>>;

/// 把关键命令的执行结果回传给协调器；ack 为 None（无需回执的命令）时是空操作。
/// 回执发送失败（接收端已退出）直接忽略：此时渲染线程本就要退出，不应因此 panic。
fn send_ack(ack: Option<Ack>, result: Result<(), String>) {
    if let Some(tx) = ack {
        let _ = tx.send(result);
    }
}

/// DirectComposition 是 COM API，renderer thread 必须自己初始化/反初始化 COM。
struct ComScope {
    should_uninitialize: bool,
}

impl ComScope {
    fn init() -> Self {
        unsafe {
            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            if hr == RPC_E_CHANGED_MODE {
                log::warn!("[Renderer] COM 已由其它并发模型初始化，跳过 CoUninitialize");
                return Self {
                    should_uninitialize: false,
                };
            }
            if hr.0 < 0 {
                log::error!("[Renderer] CoInitializeEx 失败: 0x{:08X}", hr.0 as u32);
                return Self {
                    should_uninitialize: false,
                };
            }
            Self {
                should_uninitialize: true,
            }
        }
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        if self.should_uninitialize {
            unsafe { CoUninitialize() };
        }
    }
}

/// 协调器 → renderer 线程的消息协议。
///
/// 所有命令都在单一渲染线程上串行处理，因此命令处理代码无需加锁；只有需要回传
/// 成功 / 失败的命令（EnsureMonitor / SetWallpaper）才携带 oneshot `Ack`。
enum RendererCommand {
    /// 建 / 更新某屏的原生表面；表面已存在时走 update_rect，不重建。
    EnsureMonitor {
        monitor_id: String,
        rect: MonitorRect,
        ack: Option<Ack>,
    },
    /// 释放某屏表面；实际资源回收由 RenderSurface::destroy / Drop 统一完成。
    DestroyMonitor { monitor_id: String },
    /// 组级换图：一次解码 + 一次纹理上传，组内所有屏共享同一个帧时钟。
    SetWallpaper {
        monitor_ids: Vec<String>,
        path: PathBuf,
        kind: WallpaperKind,
        fit_mode: FitMode,
        /// 解码目标尺寸：independent/mirror 为单屏，extend 为整组 union。
        /// 交给 turbojpeg 做 DCT 缩放，避免高分辨率图全量 IDCT。
        target_width: u32,
        target_height: u32,
        /// Native↔Native 且 wid 变化才为 true；跨 backend / 首帧硬切。
        crossfade: bool,
        ack: Option<Ack>,
    },
    /// 全组更新 extend 裁剪参数；None 表示该屏退回各自渲染（M4 语义 / D2 降级）。
    SetGroupGeometry {
        viewports: Vec<(String, Option<ExtendViewport>)>,
    },
    /// 单屏 fit 变更；组级裁剪走 SetGroupGeometry，二者语义不同。
    SetFitMode {
        monitor_id: String,
        fit_mode: FitMode,
    },
    /// 清掉某屏壁纸及 transition / pending 队列，回到默认桌面。
    ClearWallpaper { monitor_id: String },
    /// 全屏暂停开关：冻结视频时钟与音频，但保留 surface 与壁纸内容。
    SetPaused { paused: bool },
    /// 全局音量（0-100）下发给所有视频播放状态；上限裁剪由入口方法负责。
    SetVolume { volume: u32 },
    /// 退出消息循环，之后统一析构 surface / device / MF 平台。
    Shutdown,
}

/// 协调器持有的渲染线程句柄：命令经 `sender` 投给单一渲染线程串行执行，
/// `worker` 供 shutdown / Drop 时 join，保证线程与 D3D11 / MF 资源有序退出。
pub struct DesktopRenderer {
    /// 命令通道发送端；RendererCommand 的所有变体都通过它下发。
    sender: Sender<RendererCommand>,
    /// 用 Option 是为了 shutdown 时能 take 走并 join，避免重复 join。
    worker: Option<JoinHandle<()>>,
}

impl DesktopRenderer {
    /// 启动名为 wallpaper-renderer 的单一渲染线程；COM / MF / D3D11 都在该线程内初始化。
    /// `app_handle` 仅用于设备丢失时直接上报 renderer-error 给前端（无需反向通道）。
    pub fn start(app_handle: tauri::AppHandle) -> Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("wallpaper-renderer".to_string())
            .spawn(move || run(receiver, app_handle))?;

        Ok(Self {
            sender,
            worker: Some(worker),
        })
    }

    /// 建原生表面并等待结果；surface 创建失败会回传，不再只落渲染线程日志。
    pub async fn ensure_monitor(&self, monitor_id: String, rect: MonitorRect) -> Result<()> {
        let (ack, rx) = oneshot::channel();
        self.sender.send(RendererCommand::EnsureMonitor {
            monitor_id,
            rect,
            ack: Some(ack),
        })?;
        rx.await
            .map_err(|_| anyhow!("渲染线程在 surface 创建完成前退出"))?
            .map_err(anyhow::Error::msg)
    }

    /// 异步下发销毁命令，不等待 surface 真正释放（由渲染线程随后完成）。
    pub fn destroy_monitor(&self, monitor_id: String) -> Result<()> {
        self.sender
            .send(RendererCommand::DestroyMonitor { monitor_id })?;
        Ok(())
    }

    /// independent 时组内只有一个 id；mirror / extend 时一次覆盖整组。
    /// 等待解码 / 纹理上传完成；失败（含视频打开失败）回传给协调器发 renderer-error。
    pub async fn set_wallpaper(
        &self,
        monitor_ids: Vec<String>,
        path: PathBuf,
        kind: WallpaperKind,
        fit_mode: FitMode,
        target_width: u32,
        target_height: u32,
        crossfade: bool,
    ) -> Result<()> {
        let (ack, rx) = oneshot::channel();
        self.sender.send(RendererCommand::SetWallpaper {
            monitor_ids,
            path,
            kind,
            fit_mode,
            target_width,
            target_height,
            crossfade,
            ack: Some(ack),
        })?;
        rx.await
            .map_err(|_| anyhow!("渲染线程在壁纸解码完成前退出"))?
            .map_err(anyhow::Error::msg)
    }

    /// 组级几何更新；viewports 中的 None 表示该屏退出拼接、退回独立 fit。
    pub fn set_group_geometry(
        &self,
        viewports: Vec<(String, Option<ExtendViewport>)>,
    ) -> Result<()> {
        self.sender
            .send(RendererCommand::SetGroupGeometry { viewports })?;
        Ok(())
    }

    /// 单屏 fit 变更；不触发解码，只影响后续帧的 viewport 计算。
    pub fn set_fit_mode(&self, monitor_id: String, fit_mode: FitMode) -> Result<()> {
        self.sender.send(RendererCommand::SetFitMode {
            monitor_id,
            fit_mode,
        })?;
        Ok(())
    }

    /// 清空该屏壁纸及 transition / pending，视频随之停止出声。
    pub fn clear_wallpaper(&self, monitor_id: String) -> Result<()> {
        self.sender
            .send(RendererCommand::ClearWallpaper { monitor_id })?;
        Ok(())
    }

    /// 全屏暂停开关；与 fade 暂停按 pause reason 合并，任一方保持暂停都不会被另一方误恢复。
    pub fn set_paused(&self, paused: bool) -> Result<()> {
        self.sender.send(RendererCommand::SetPaused { paused })?;
        Ok(())
    }

    /// 下发全局音量并夹到 0-100；渲染线程按 Arc 去重后逐个设置视频音量。
    pub fn set_volume(&self, volume: u32) -> Result<()> {
        self.sender.send(RendererCommand::SetVolume {
            volume: volume.min(100),
        })?;
        Ok(())
    }

    /// 先发 Shutdown 再 join；若通道已断（线程已退出）则直接返回。
    pub fn shutdown(&mut self) {
        if self.sender.send(RendererCommand::Shutdown).is_err() {
            return;
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// 兜底 join：调用方忘记 shutdown 时也不会把渲染线程和 D3D11 / MF 资源留在后台。
impl Drop for DesktopRenderer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// 渲染线程主循环。
///
/// 单次 tick：排空 command → 泵 WM_QUIT → 裁决视频暂停 / 音频资格 →
/// 推进组级视频时钟与共享帧 → 逐 surface 绘制并结束 fade → 睡眠一帧。
/// 暂停时跳过绘制但保留 surface 与内容，恢复后无需重新解码。
fn run(receiver: Receiver<RendererCommand>, app_handle: tauri::AppHandle) {
    let _com = ComScope::init();

    // Media Foundation 的 SourceReader 依赖 MF 平台已启动；失败时图片链路照常工作，
    // 只有视频会打开失败并把错误回传给协调器。
    let mf_started = match mf_startup() {
        Ok(()) => true,
        Err(err) => {
            log::error!(
                "[Renderer] Media Foundation 初始化失败，视频壁纸不可用: {:#}",
                err
            );
            false
        }
    };

    // 所有 surface 共用一个 device；SRV 不能跨 device 绑定，而 extend 组要求同纹理同帧。
    // Option 是为了设备丢失时整体 drop（连带 video worker join），下次命令再惰性重建。
    let mut shared = Some(match SharedDevice::new() {
        Ok(device) => device,
        Err(err) => {
            log::error!("共享 D3D11 上下文创建失败: {:#}", err);
            return;
        }
    });

    let mut surfaces: HashMap<String, RenderSurface> = HashMap::new();
    // 同一张图会因「首次双下发 / 同步模式逐屏下发 / A→B→A 回切」多次到达，
    // 缓存整条解码 + 纹理上传链。命中时连 started_at 一起复用：动图回切会接着播，
    // 组内所有 surface 也因此始终持有同一个帧时钟，不会出现跨屏错帧。
    let mut frame_cache = FrameCache::<SharedFrames>::with_default_budget();
    // 一次性图片解码 worker 的结果回收通道；worker 只借用 device clone，完成后即退出。
    let (decode_tx, decode_rx) = mpsc::channel::<DecodeResult>();
    let mut pending_decodes: HashMap<u64, PendingDecode> = HashMap::new();
    // cache key → 在途 generation：同 key 的重复命令合并到同一次解码，
    // 避免「首次双下发」时把同一张图解两遍。
    let mut inflight_by_key: HashMap<CacheKey, u64> = HashMap::new();
    let mut next_generation = 0u64;
    let mut running = true;
    // 全屏暂停标志：只冻结时钟 / 音频，不销毁内容；与 fade 暂停按 reason 各自独立。
    let mut paused = false;
    // global_volume 默认 0；启动时由 RenderCoordinator 再同步一次 DB 中的值。
    let mut volume = 0u32;

    while running {
        while let Ok(command) = receiver.try_recv() {
            match command {
                RendererCommand::EnsureMonitor {
                    monitor_id,
                    rect,
                    ack,
                } => {
                    if let Some(surface) = surfaces.get_mut(&monitor_id) {
                        if let Err(err) = surface.update_rect(rect) {
                            let message = format!("原生壁纸表面更新失败 {}: {:#}", monitor_id, err);
                            log::error!("{}", message);
                            send_ack(ack, Err(message));
                        } else {
                            send_ack(ack, Ok(()));
                        }
                        continue;
                    }
                    match ensure_shared(&mut shared).and_then(|s| RenderSurface::new(s, rect)) {
                        Ok(surface) => {
                            surfaces.insert(monitor_id, surface);
                            send_ack(ack, Ok(()));
                        }
                        Err(err) => {
                            let message = format!("原生壁纸表面创建失败: {:#}", err);
                            log::error!("{}", message);
                            send_ack(ack, Err(message));
                        }
                    }
                }
                RendererCommand::DestroyMonitor { monitor_id } => {
                    // 该屏已销毁，任何在途解码结果挂上去都没有意义；generation 递增让它过期。
                    next_generation += 1;
                    if let Some(mut surface) = surfaces.remove(&monitor_id) {
                        surface.destroy();
                    }
                }
                RendererCommand::SetWallpaper {
                    monitor_ids,
                    path,
                    kind,
                    fit_mode,
                    target_width,
                    target_height,
                    crossfade,
                    ack,
                } => {
                    // 每条换图命令递增 generation：连点 / 清屏 / 销毁显示器时，
                    // 让所有更早发出的在途解码结果自动过期（见结果回收处）。
                    next_generation += 1;
                    let generation = next_generation;

                    if kind == WallpaperKind::Video {
                        // 视频仍走同步流式打开：首帧等待是有意设计，保证 attach 时已可绘制。
                        let identity = VideoIdentity::from_path(&path);
                        let result =
                            if let Some(existing) = find_reusable_video(&surfaces, &identity) {
                                log::info!(
                                    "[Renderer] 复用已有视频播放状态 {:?}",
                                    path.file_name().unwrap_or_default()
                                );
                                Ok(WallpaperMedia::Video(existing))
                            } else {
                                ensure_shared(&mut shared)
                                    .and_then(|s| s.open_video(&path, volume))
                                    .map(WallpaperMedia::Video)
                                    .with_context(|| format!("视频解码失败 {:?}", path))
                            };
                        match result {
                            Ok(media) => attach_media(
                                &mut surfaces,
                                &monitor_ids,
                                media,
                                fit_mode,
                                crossfade,
                                Instant::now(),
                                ack,
                            ),
                            Err(err) => {
                                let message = format!("{:#}", err);
                                log::error!("[Renderer] {}", message);
                                send_ack(ack, Err(message));
                            }
                        }
                        continue;
                    }

                    // 图片：缓存命中直接 attach（复用同一份 Arc 与帧时钟）；
                    // miss 才 spawn 一次性 worker，渲染线程立即继续下一帧，不冻结旧画面。
                    let target = Some((target_width, target_height));
                    let key = cache_key_with_target(&path, target);
                    if let Some(frames) = key.as_ref().and_then(|key| frame_cache.get(key)) {
                        attach_media(
                            &mut surfaces,
                            &monitor_ids,
                            WallpaperMedia::Frames(frames),
                            fit_mode,
                            crossfade,
                            Instant::now(),
                            ack,
                        );
                        continue;
                    }

                    // 同一张图可能连续下发两次（update_wallpaper 与同步窗口各一次），
                    // 或首屏还没解码完就再次下发：合并到同一次在途解码，避免重复 spawn。
                    // 多屏 monitor_ids 取并集，所有等待方共享同一份结果。
                    if let Some(inflight_gen) = key
                        .as_ref()
                        .and_then(|key| inflight_by_key.get(key).copied())
                        .filter(|gen| *gen == next_generation)
                    {
                        if let Some(pending) = pending_decodes.get_mut(&inflight_gen) {
                            for monitor_id in monitor_ids {
                                if !pending.monitor_ids.contains(&monitor_id) {
                                    pending.monitor_ids.push(monitor_id);
                                }
                            }
                            if let Some(ack) = ack {
                                pending.acks.push(ack);
                            }
                            continue;
                        }
                    }

                    let device = match ensure_shared(&mut shared) {
                        Ok(s) => s.device().clone(),
                        Err(err) => {
                            let message = format!("重建 D3D11 上下文失败: {:#}", err);
                            log::error!("[Renderer] {}", message);
                            send_ack(ack, Err(message));
                            continue;
                        }
                    };
                    let worker_path = path.clone();
                    let worker_target = target;
                    let tx = decode_tx.clone();
                    let handle = thread::spawn(move || {
                        let result = image::decode_and_upload(&device, &worker_path, worker_target)
                            .map_err(|err| format!("{:#}", err));
                        let _ = tx.send(DecodeResult { generation, result });
                    });
                    if let Some(key) = key.clone() {
                        inflight_by_key.insert(key, generation);
                    }
                    pending_decodes.insert(
                        generation,
                        PendingDecode {
                            key,
                            monitor_ids,
                            fit_mode,
                            crossfade,
                            acks: ack.into_iter().collect(),
                            handle,
                        },
                    );
                }
                RendererCommand::SetGroupGeometry { viewports } => {
                    for (monitor_id, viewport) in viewports {
                        if let Some(surface) = surfaces.get_mut(&monitor_id) {
                            surface.set_extend_viewport(viewport);
                        }
                    }
                }
                RendererCommand::SetFitMode {
                    monitor_id,
                    fit_mode,
                } => {
                    if let Some(surface) = surfaces.get_mut(&monitor_id) {
                        surface.set_fit_mode(fit_mode);
                    }
                }
                RendererCommand::ClearWallpaper { monitor_id } => {
                    // 清屏后迟到的解码结果不应把壁纸盖回来。
                    next_generation += 1;
                    if let Some(surface) = surfaces.get_mut(&monitor_id) {
                        surface.clear_wallpaper();
                    }
                }
                RendererCommand::SetPaused { paused: next } => {
                    paused = next;
                    // 冻结 / 恢复所有视频（含 fade 队列里的 to）的播放时钟与音频；
                    // 不 flush、不重建 SourceReader。按 Arc 指针去重，避免组内重复记暂停时长。
                    let now = Instant::now();
                    let mut seen = std::collections::HashSet::new();
                    for surface in surfaces.values_mut() {
                        for playback in surface.video_arcs() {
                            if !seen.insert(Arc::as_ptr(playback)) {
                                continue;
                            }
                            if let Ok(mut guard) = playback.lock() {
                                guard.set_pause_reason(PAUSE_FULLSCREEN, next, now);
                            }
                        }
                    }
                }
                RendererCommand::SetVolume { volume: next } => {
                    volume = next;
                    let mut seen = std::collections::HashSet::new();
                    for surface in surfaces.values_mut() {
                        for playback in surface.video_arcs() {
                            if !seen.insert(Arc::as_ptr(playback)) {
                                continue;
                            }
                            if let Ok(mut guard) = playback.lock() {
                                guard.set_audio_volume(volume);
                            }
                        }
                    }
                }
                RendererCommand::Shutdown => running = false,
            }
        }

        // 回收一次性解码结果。只有 generation 仍等于最新值的那份才算数：连点、
        // 清屏、销毁显示器都会递增 generation 让旧结果过期。过期结果静默丢弃，
        // ack 回 Ok——它不是解码失败，不该让协调器误报 renderer-error。
        while let Ok(DecodeResult { generation, result }) = decode_rx.try_recv() {
            let Some(pending) = pending_decodes.remove(&generation) else {
                continue;
            };
            // worker 已经发回结果，join 只做线程资源回收，不会阻塞。
            let _ = pending.handle.join();
            if let Some(key) = pending.key.as_ref() {
                inflight_by_key.remove(key);
            }

            if generation != next_generation {
                log::info!(
                    "[Renderer] 图片解码结果过期丢弃 generation={} latest={}",
                    generation,
                    next_generation
                );
                send_acks(pending.acks, Ok(()));
                continue;
            }

            match result {
                Ok(frames) => {
                    // 帧时钟起点对齐到真正 attach 的时刻，保证 GIF 从第一帧开始、
                    // 组内所有屏共享同一条时间线；缓存命中路径复用旧 Arc 不进这里。
                    let now = Instant::now();
                    let mut frames = frames;
                    frames.reset_clock(now);
                    let frames = Arc::new(frames);
                    if let Some(key) = pending.key {
                        frame_cache.insert(key, frames.clone());
                    }
                    // attach 只回一个 ack：它是「内容已挂载」的语义代表，
                    // 其余等待方由 send_acks 统一回 Ok。
                    let mut acks = pending.acks;
                    let primary = acks.pop();
                    attach_media(
                        &mut surfaces,
                        &pending.monitor_ids,
                        WallpaperMedia::Frames(frames),
                        pending.fit_mode,
                        pending.crossfade,
                        now,
                        primary,
                    );
                    send_acks(acks, Ok(()));
                }
                Err(message) => {
                    log::error!("[Renderer] {}", message);
                    send_acks(pending.acks, Err(message));
                }
            }
        }

        if !pump_messages() {
            break;
        }

        // 音频播放资格与视频绘制解耦：全屏暂停 / fade 冻结 / pending 队列统一在这里裁决。
        // 必须在绘制前调用，避免 to 视频在 fade 结束前提前出声，或恢复全屏时 pending 漏音。
        sync_video_pause(&surfaces, Instant::now());
        sync_audio_state(&surfaces, paused);

        // 全屏暂停时跳过绘制，但保留 surface 与壁纸内容，恢复后无需重新解码
        if !paused {
            let now = Instant::now();
            // 组级视频时钟推进：同一 Arc 只 advance 一次，避免每屏各推进一次导致快进。
            advance_videos(&mut surfaces, now);
            // 帧级共享纹理同步：同一 VideoPlayback 每帧只 AcquireSync/CopyResource 一次，
            // 所有 surface 随后只读 renderer 本地副本，避免双屏抢 keyed mutex 闪烁。
            begin_video_frames(&surfaces);
            // fade 结束时按 Arc 指针去重：组内共享同一播放状态时只 reset 一次，
            // 否则多屏会对同一个 SourceReader 重复 Flush / seek。
            let mut reset_seen: HashSet<*const Mutex<VideoPlayback>> = HashSet::new();
            // 任一 surface 报设备丢失就整组清理；D3D11 device 全局唯一，
            // 一块屏失效意味着所有屏的 SRV / swapchain 与 video session GPU 同时失效。
            let mut device_lost = false;
            for (monitor_id, surface) in surfaces.iter_mut() {
                if surface.render(monitor_id, now) {
                    device_lost = true;
                }
                if device_lost {
                    continue;
                }
                if surface.finish_transition(now) {
                    // 刚提升为 wallpaper 的就是 fade 的 to。组内共享同一个播放状态，
                    // 只能 reset 一次；否则多屏会重复 Flush / seek 同一个 SourceReader。
                    if let Some(playback) = surface.video() {
                        let ptr = Arc::as_ptr(playback);
                        if reset_seen.insert(ptr) {
                            if let Ok(mut guard) = playback.lock() {
                                if let Err(err) = guard.reset_clock(now) {
                                    log::error!(
                                        "[Renderer] fade 结束后视频时钟重置失败: {:#}",
                                        err
                                    );
                                }
                            }
                        }
                    }
                }
            }
            // fade 已在上面结束；立即同步一次，让新 wallpaper 恢复播放。
            // 若 finish_transition 又启动了 pending 下一段，仍会保持 PAUSE_FADE。
            sync_video_pause(&surfaces, now);
            sync_audio_state(&surfaces, paused);

            if device_lost {
                // 设备丢失：整组清理失效的 GPU 资源，保留 renderer 线程本身。
                // 不主动重建——下次 EnsureMonitor / SetWallpaper 命令到达时由
                // ensure_shared 惰性重建（连带新 video worker）并重挂壁纸。
                log::error!("[Renderer] 检测到设备丢失，清理全部原生 surface 与共享 device");
                for (_, mut surface) in surfaces.drain() {
                    surface.destroy();
                }
                // drop SharedDevice 会连带 drop VideoWorkerHost：其内部 Drop 发
                // Shutdown 并 join worker，避免失效 device 上的线程残留。
                shared = None;
                // 缓存里的 SRV 全部指向失效 device，必须一并丢弃。
                frame_cache.clear();
                // 在途解码结果持有的 device clone 也随设备失效，但 worker 是一次性的，
                // 结果回来后 attach 时 surface 已不存在，会被 attach_media 判为无目标；
                // 这里主动递增 generation 让它们过期，避免在新 device 上挂旧纹理。
                next_generation += 1;
                let payload = RendererErrorPayload {
                    monitor_id: None,
                    wallpaper_id: None,
                    code: RendererErrorCode::DeviceLost.as_str().to_string(),
                    message: "显卡设备丢失，原生壁纸已清理；下次换图将自动恢复".to_string(),
                };
                if let Err(err) = app_handle.typed_emit(&payload) {
                    log::warn!("[Renderer] DeviceLost 事件发送失败: {}", err);
                }
                // M19：WebView2 同样走 GPU 合成，设备丢失时一并释放其壁纸窗口。
                // 这里在渲染线程内，取 AppContext 只能经 AppHandle 的 state，
                // 且 window_manager 是 tokio Mutex，故 spawn 到 async runtime 清理。
                // 不通知前端：前端显示器参数已降级为只读，窗口销毁后由下一次
                // ensure_target 命令重建（M20 热插拔事件化后统一处理）。
                let cleanup_handle = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    let app_ctx = cleanup_handle.state::<crate::ctx::AppContext>();
                    app_ctx.window_manager.lock().await.destroy_all();
                    log::info!("[Renderer] DeviceLost 已清理全部 WebView 壁纸窗口");
                });
            }
        }

        // 帧率自适应：纯静态壁纸没有逐帧变化，降到低频唤醒即可；
        // 视频 / fade / 动图仍按 16ms 推进，保证帧时钟与淡入淡出平滑。
        // 命令响应最差等于 STALE_TICK_MS，静态场景换图的 decode 成本远大于此。
        const STALE_TICK_MS: u64 = 100;
        let has_motion = !paused
            && surfaces
                .values()
                .any(RenderSurface::needs_continuous_frames);
        thread::sleep(Duration::from_millis(if has_motion {
            16
        } else {
            STALE_TICK_MS
        }));
    }

    for (_, mut surface) in surfaces {
        surface.destroy();
    }
    // 退出时可能还有在途图片解码：先 join 让它们释放 device clone，
    // 且对等待中的 ack 回 Err，避免协调器永久挂住。
    for (_, pending) in pending_decodes {
        let _ = pending.handle.join();
        send_acks(
            pending.acks,
            Err("渲染线程退出，图片解码未完成".to_string()),
        );
    }
    // 所有 SourceReader / worker 已随 surface 释放；先释放 D3D11，再关闭 MF 平台，
    // 保证 MFStartup / MFShutdown 成对，且不会从工作队列线程调用 shutdown。
    drop(frame_cache);
    drop(decode_tx);
    drop(decode_rx);
    drop(shared);
    if mf_started {
        if let Err(err) = mf_shutdown() {
            log::warn!("[Renderer] Media Foundation 关闭失败: {:#}", err);
        }
    }
}

/// 同一份视频在 current / transition / pending 中已经存在时直接复用，不再开新 worker。
fn find_reusable_video(
    surfaces: &HashMap<String, RenderSurface>,
    identity: &VideoIdentity,
) -> Option<Arc<Mutex<VideoPlayback>>> {
    let mut seen: HashSet<*const Mutex<VideoPlayback>> = HashSet::new();
    for surface in surfaces.values() {
        for playback in surface.video_arcs() {
            let ptr = Arc::as_ptr(playback);
            if !seen.insert(ptr) {
                continue;
            }
            if let Ok(guard) = playback.lock() {
                if guard.identity() == identity {
                    return Some(playback.clone());
                }
            }
        }
    }
    None
}

/// 每帧对所有唯一 VideoPlayback 做一次共享帧刷新。
///
/// Mirror/Extend 多屏共享同一个 Arc<Mutex<VideoPlayback>>；必须按指针去重，
/// 否则每个 surface 各抢一次 keyed mutex，会出现一块屏拿到帧、另一块屏拿到 key0 的黑屏。
fn begin_video_frames(surfaces: &HashMap<String, RenderSurface>) {
    let mut seen: HashSet<*const Mutex<VideoPlayback>> = HashSet::new();
    for surface in surfaces.values() {
        for playback in surface.video_arcs() {
            let ptr = Arc::as_ptr(playback);
            if !seen.insert(ptr) {
                continue;
            }
            if let Ok(mut guard) = playback.lock() {
                guard.begin_frame();
            }
        }
    }
}

/// fade / pending 期间暂停所有进程的视频源；fade 结束后恢复仍在正常显示的状态。
/// 与全屏暂停按 pause reason 合并，避免一方解除时误恢复另一方。
fn sync_video_pause(surfaces: &HashMap<String, RenderSurface>, now: Instant) {
    // 只要有任意一屏处于 fade / pending，就把它持有的全部视频（含 to）记为冻结，
    // 整段过渡期间 from / to 的播放时钟都停住，避免中途继续走帧。
    let mut fade_frozen: HashSet<*const Mutex<VideoPlayback>> = HashSet::new();
    for surface in surfaces.values() {
        if surface.fade_active_or_pending() {
            for playback in surface.video_arcs() {
                fade_frozen.insert(Arc::as_ptr(playback));
            }
        }
    }

    let mut seen = HashSet::new();
    for surface in surfaces.values() {
        for playback in surface.video_arcs() {
            let ptr = Arc::as_ptr(playback);
            if !seen.insert(ptr) {
                continue;
            }
            if let Ok(mut guard) = playback.lock() {
                guard.set_pause_reason(PAUSE_FADE, fade_frozen.contains(&ptr), now);
            }
        }
    }
}

/// 裁决每个视频的音频是否应该出声。
///
/// 只有「当前正在显示、且不在 fade / pending 中、且未全屏暂停」的播放状态才播放音频。
/// 这样 Mirror/Extend 组共享一个 VideoPlayback 时自然只有一份音频；Independent 每屏
/// 各自持有一个 VideoPlayback，自然按屏叠加。与 WebView 的过渡语义保持一致。
fn sync_audio_state(surfaces: &HashMap<String, RenderSurface>, paused: bool) {
    // frozen：处于 fade / pending 的屏其上全部视频都静音（含尚未显示的 to）。
    // active：只有正常显示（无 fade）的当前壁纸视频才取得出声资格。
    let mut frozen: HashSet<*const Mutex<VideoPlayback>> = HashSet::new();
    let mut active: HashSet<*const Mutex<VideoPlayback>> = HashSet::new();

    for surface in surfaces.values() {
        if surface.fade_active_or_pending() {
            for playback in surface.video_arcs() {
                frozen.insert(Arc::as_ptr(playback));
            }
        } else if let Some(playback) = surface.video() {
            active.insert(Arc::as_ptr(playback));
        }
    }

    let mut seen = HashSet::new();
    for surface in surfaces.values() {
        for playback in surface.video_arcs() {
            let ptr = Arc::as_ptr(playback);
            if !seen.insert(ptr) {
                continue;
            }
            let should_play = !paused && active.contains(&ptr) && !frozen.contains(&ptr);
            if let Ok(mut guard) = playback.lock() {
                guard.set_audio_paused(!should_play);
            }
        }
    }
}

/// 推进整组视频时钟：按 Arc 指针去重，同一播放状态每帧只 advance 一次。
/// 暂停状态由 renderer loop 的 `paused` 标志保证不会走到这里。
/// fade 期间 from / to 都冻结，to 为视频时 fade 结束才由 finish_transition 重置时钟。
fn advance_videos(surfaces: &mut HashMap<String, RenderSurface>, now: Instant) {
    // 先收集冻结集合：组内共享同一个 Arc，只要有一屏在 fade 就不能推进，
    // 否则未同步的那一屏会把整个组的 from 视频偷偷往前推。
    let mut frozen: HashSet<*const Mutex<VideoPlayback>> = HashSet::new();
    for surface in surfaces.values() {
        if surface.fade_active_or_pending() {
            if let Some(playback) = surface.video() {
                frozen.insert(Arc::as_ptr(playback));
            }
        }
    }

    let mut seen: HashSet<*const Mutex<VideoPlayback>> = HashSet::new();
    for surface in surfaces.values_mut() {
        let Some(playback) = surface.video() else {
            continue;
        };
        let ptr = Arc::as_ptr(playback);
        if frozen.contains(&ptr) || !seen.insert(ptr) {
            continue;
        }
        if let Ok(mut guard) = playback.lock() {
            if let Err(err) = guard.advance(now) {
                log::error!("[Renderer] 视频播放推进失败: {:#}", err);
            }
        }
    }
}

/// 排空线程消息队列，仅关心 WM_QUIT：收到即返回 false 让主循环退出。
/// DirectComposition 合成不需要窗口过程做额外绘制，因此消息泵极简。
fn pump_messages() -> bool {
    unsafe {
        let mut message = MSG::default();
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            if message.message == WM_QUIT {
                log::warn!("[Renderer] 收到 WM_QUIT，渲染线程准备退出");
                return false;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    true
}
