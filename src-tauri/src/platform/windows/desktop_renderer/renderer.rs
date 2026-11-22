use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use tokio::sync::oneshot;
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_QUIT,
};

use super::surface::{RenderSurface, SharedDevice, SharedFrames, WallpaperMedia};
use super::video::{startup as mf_startup, VideoPlayback};
use crate::platform::wallpaper_renderer::frame_cache::{cache_key, FrameCache};
use crate::platform::wallpaper_renderer::{ExtendViewport, FitMode, MonitorRect, WallpaperKind};

/// 关键命令的异步回执：把 renderer 线程内的解码 / surface 失败回传给协调器。
/// 只用于 EnsureMonitor / SetWallpaper——这两条会真正做 D3D11 资源创建与解码。
type Ack = oneshot::Sender<Result<(), String>>;

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

enum RendererCommand {
    EnsureMonitor {
        monitor_id: String,
        rect: MonitorRect,
        ack: Option<Ack>,
    },
    DestroyMonitor {
        monitor_id: String,
    },
    /// 组级换图：一次解码 + 一次纹理上传，组内所有屏共享同一个帧时钟。
    SetWallpaper {
        monitor_ids: Vec<String>,
        path: PathBuf,
        kind: WallpaperKind,
        fit_mode: FitMode,
        ack: Option<Ack>,
    },
    /// 全组更新 extend 裁剪参数；None 表示该屏退回各自渲染（M4 语义 / D2 降级）。
    SetGroupGeometry {
        viewports: Vec<(String, Option<ExtendViewport>)>,
    },
    SetFitMode {
        monitor_id: String,
        fit_mode: FitMode,
    },
    ClearWallpaper {
        monitor_id: String,
    },
    SetPaused {
        paused: bool,
    },
    Shutdown,
}

pub struct DesktopRenderer {
    sender: Sender<RendererCommand>,
    worker: Option<JoinHandle<()>>,
}

impl DesktopRenderer {
    pub fn start() -> Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("wallpaper-renderer".to_string())
            .spawn(move || run(receiver))?;

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
    ) -> Result<()> {
        let (ack, rx) = oneshot::channel();
        self.sender.send(RendererCommand::SetWallpaper {
            monitor_ids,
            path,
            kind,
            fit_mode,
            ack: Some(ack),
        })?;
        rx.await
            .map_err(|_| anyhow!("渲染线程在壁纸解码完成前退出"))?
            .map_err(anyhow::Error::msg)
    }

    pub fn set_group_geometry(
        &self,
        viewports: Vec<(String, Option<ExtendViewport>)>,
    ) -> Result<()> {
        self.sender
            .send(RendererCommand::SetGroupGeometry { viewports })?;
        Ok(())
    }

    pub fn set_fit_mode(&self, monitor_id: String, fit_mode: FitMode) -> Result<()> {
        self.sender.send(RendererCommand::SetFitMode {
            monitor_id,
            fit_mode,
        })?;
        Ok(())
    }

    pub fn clear_wallpaper(&self, monitor_id: String) -> Result<()> {
        self.sender
            .send(RendererCommand::ClearWallpaper { monitor_id })?;
        Ok(())
    }

    pub fn set_paused(&self, paused: bool) -> Result<()> {
        self.sender.send(RendererCommand::SetPaused { paused })?;
        Ok(())
    }

    pub fn shutdown(&mut self) {
        if self.sender.send(RendererCommand::Shutdown).is_err() {
            return;
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for DesktopRenderer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run(receiver: Receiver<RendererCommand>) {
    let _com = ComScope::init();

    // Media Foundation 的 SourceReader 依赖 MF 平台已启动；失败时图片链路照常工作，
    // 只有视频会打开失败并把错误回传给协调器。
    if let Err(err) = mf_startup() {
        log::error!(
            "[Renderer] Media Foundation 初始化失败，视频壁纸不可用: {:#}",
            err
        );
    }

    // 所有 surface 共用一个 device；SRV 不能跨 device 绑定，而 extend 组要求同纹理同帧。
    let shared = match SharedDevice::new() {
        Ok(device) => device,
        Err(err) => {
            log::error!("共享 D3D11 上下文创建失败: {:#}", err);
            return;
        }
    };

    let mut surfaces: HashMap<String, RenderSurface> = HashMap::new();
    // 同一张图会因「首次双下发 / 同步模式逐屏下发 / A→B→A 回切」多次到达，
    // 缓存整条解码 + 纹理上传链。命中时连 started_at 一起复用：动图回切会接着播，
    // 组内所有 surface 也因此始终持有同一个帧时钟，不会出现跨屏错帧。
    let mut frame_cache = FrameCache::<SharedFrames>::with_default_budget();
    let mut running = true;
    let mut paused = false;

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
                    match RenderSurface::new(&shared, rect) {
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
                    if let Some(mut surface) = surfaces.remove(&monitor_id) {
                        surface.destroy();
                    }
                }
                RendererCommand::SetWallpaper {
                    monitor_ids,
                    path,
                    kind,
                    fit_mode,
                    ack,
                } => {
                    // 视频走流式：一次打开、组内共享一个 Arc；图片/GIF/WebP 走缓存。
                    // 两条链路都必须先构造出媒体内容，再挂到每个 surface，避免半挂状态。
                    let result: Result<WallpaperMedia> = match kind {
                        WallpaperKind::Video => shared
                            .open_video(&path)
                            .map(WallpaperMedia::Video)
                            .with_context(|| format!("视频解码失败 {:?}", path)),
                        _ => frame_cache
                            .get_or_insert_with(cache_key(&path), || {
                                shared.upload_frames(&path, kind)
                            })
                            .map(WallpaperMedia::Frames)
                            .with_context(|| format!("图片解码失败 {:?}", path)),
                    };

                    match result {
                        Ok(media) => {
                            let mut attached = false;
                            for monitor_id in &monitor_ids {
                                if let Some(surface) = surfaces.get_mut(monitor_id) {
                                    match &media {
                                        WallpaperMedia::Frames(frames) => {
                                            surface.set_frames(frames.clone(), fit_mode)
                                        }
                                        WallpaperMedia::Video(playback) => {
                                            surface.set_video(playback.clone(), fit_mode)
                                        }
                                    }
                                    attached = true;
                                } else {
                                    log::warn!("[Renderer] {} 组换图时原生表面不存在", monitor_id);
                                }
                            }
                            if attached {
                                send_ack(ack, Ok(()));
                            } else {
                                let message =
                                    format!("组换图时所有原生表面都不存在: {:?}", monitor_ids);
                                log::warn!("[Renderer] {}", message);
                                send_ack(ack, Err(message));
                            }
                        }
                        Err(err) => {
                            let message = format!("{:#}", err);
                            log::error!("[Renderer] {}", message);
                            send_ack(ack, Err(message));
                        }
                    }
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
                    if let Some(surface) = surfaces.get_mut(&monitor_id) {
                        surface.clear_wallpaper();
                    }
                }
                RendererCommand::SetPaused { paused: next } => {
                    paused = next;
                    // 冻结 / 恢复组内每个视频的播放时钟；不 flush、不重建 SourceReader。
                    // 按 Arc 指针去重，同一个播放状态只记一次暂停时长。
                    let now = Instant::now();
                    let mut seen = std::collections::HashSet::new();
                    for surface in surfaces.values_mut() {
                        let Some(playback) = surface.video() else {
                            continue;
                        };
                        if !seen.insert(Arc::as_ptr(playback)) {
                            continue;
                        }
                        if let Ok(mut guard) = playback.lock() {
                            guard.set_paused(next, now);
                        }
                    }
                }
                RendererCommand::Shutdown => running = false,
            }
        }

        if !pump_messages() {
            break;
        }

        // 全屏暂停时跳过绘制，但保留 surface 与壁纸内容，恢复后无需重新解码
        if !paused {
            // 组级视频时钟推进：同一 Arc 只 advance 一次，避免每屏各推进一次导致快进。
            advance_videos(&mut surfaces, Instant::now());
            for (monitor_id, surface) in surfaces.iter_mut() {
                surface.render(monitor_id);
            }
        }

        thread::sleep(Duration::from_millis(16));
    }

    for (_, mut surface) in surfaces {
        surface.destroy();
    }
}

/// 推进整组视频时钟：按 Arc 指针去重，同一播放状态每帧只 advance 一次。
/// 暂停状态由 renderer loop 的 `paused` 标志保证不会走到这里。
fn advance_videos(surfaces: &mut HashMap<String, RenderSurface>, now: Instant) {
    use std::collections::HashSet;
    use std::sync::Arc;

    let mut seen: HashSet<*const Mutex<VideoPlayback>> = HashSet::new();
    for surface in surfaces.values_mut() {
        let Some(playback) = surface.video() else {
            continue;
        };
        if !seen.insert(Arc::as_ptr(playback)) {
            continue;
        }
        if let Ok(mut guard) = playback.lock() {
            if let Err(err) = guard.advance(now) {
                log::error!("[Renderer] 视频播放推进失败: {:#}", err);
            }
        }
    }
}

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
