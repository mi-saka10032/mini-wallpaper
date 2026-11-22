use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::Result;
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_QUIT,
};

use super::surface::{RenderSurface, SharedDevice};
use crate::platform::wallpaper_renderer::{ExtendViewport, FitMode, MonitorRect, WallpaperKind};

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

    pub fn ensure_monitor(&self, monitor_id: String, rect: MonitorRect) -> Result<()> {
        self.sender
            .send(RendererCommand::EnsureMonitor { monitor_id, rect })?;
        Ok(())
    }

    pub fn destroy_monitor(&self, monitor_id: String) -> Result<()> {
        self.sender
            .send(RendererCommand::DestroyMonitor { monitor_id })?;
        Ok(())
    }

    /// independent 时组内只有一个 id；mirror / extend 时一次覆盖整组。
    pub fn set_wallpaper(
        &self,
        monitor_ids: Vec<String>,
        path: PathBuf,
        kind: WallpaperKind,
        fit_mode: FitMode,
    ) -> Result<()> {
        self.sender.send(RendererCommand::SetWallpaper {
            monitor_ids,
            path,
            kind,
            fit_mode,
        })?;
        Ok(())
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

    // 所有 surface 共用一个 device；SRV 不能跨 device 绑定，而 extend 组要求同纹理同帧。
    let shared = match SharedDevice::new() {
        Ok(device) => device,
        Err(err) => {
            log::error!("共享 D3D11 上下文创建失败: {:#}", err);
            return;
        }
    };

    let mut surfaces: HashMap<String, RenderSurface> = HashMap::new();
    let mut running = true;
    let mut paused = false;

    while running {
        while let Ok(command) = receiver.try_recv() {
            match command {
                RendererCommand::EnsureMonitor { monitor_id, rect } => {
                    if let Some(surface) = surfaces.get_mut(&monitor_id) {
                        if let Err(err) = surface.update_rect(rect) {
                            log::error!("原生壁纸表面更新失败 {}: {:#}", monitor_id, err);
                        }
                        continue;
                    }
                    match RenderSurface::new(&shared, rect) {
                        Ok(surface) => {
                            surfaces.insert(monitor_id, surface);
                        }
                        Err(err) => log::error!("原生壁纸表面创建失败: {:#}", err),
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
                } => match shared.upload_frames(&path, kind) {
                    Ok(frames) => {
                        for monitor_id in monitor_ids {
                            if let Some(surface) = surfaces.get_mut(&monitor_id) {
                                surface.set_frames(frames.clone(), fit_mode);
                            } else {
                                log::warn!("[Renderer] {} 组换图时原生表面不存在", monitor_id);
                            }
                        }
                    }
                    Err(err) => log::error!("原生壁纸解码失败 {:?}: {:#}", path, err),
                },
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
                }
                RendererCommand::Shutdown => running = false,
            }
        }

        if !pump_messages() {
            break;
        }

        // 全屏暂停时跳过绘制，但保留 surface 与壁纸内容，恢复后无需重新解码
        if !paused {
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
