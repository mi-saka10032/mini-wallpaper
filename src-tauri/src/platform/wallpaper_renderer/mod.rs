#![allow(dead_code, unused_imports)]

use std::path::PathBuf;

use anyhow::Result;

mod contracts;
pub mod decoder;
pub mod fit;
mod selection;

pub use contracts::{
    BackendSelection, DisplayMode, ExtendViewport, FitMode, MonitorRect, RenderBackend,
    RenderDescriptor, RendererError, RendererErrorCode, WallpaperKind,
};
pub use selection::{default_backend_selection, resolve_backend};

/// 跨平台渲染器门面：Windows 下是真实的 D3D11 渲染线程，其它平台为空实现。
///
/// 让上层 RenderCoordinator 无需 cfg 分支即可统一调用，macOS 开发/CI 下也能编译通过。
#[cfg(target_os = "windows")]
pub use crate::platform::windows::desktop_renderer::DesktopRenderer;

#[cfg(not(target_os = "windows"))]
pub struct DesktopRenderer;

#[cfg(not(target_os = "windows"))]
impl DesktopRenderer {
    pub fn start() -> Result<Self> {
        Ok(Self)
    }

    pub fn ensure_monitor(&self, _monitor_id: String, _rect: MonitorRect) -> Result<()> {
        Ok(())
    }

    pub fn destroy_monitor(&self, _monitor_id: String) -> Result<()> {
        Ok(())
    }

    pub fn set_wallpaper(
        &self,
        _monitor_id: String,
        _path: PathBuf,
        _kind: WallpaperKind,
        _fit_mode: FitMode,
    ) -> Result<()> {
        Ok(())
    }

    pub fn set_fit_mode(&self, _monitor_id: String, _fit_mode: FitMode) -> Result<()> {
        Ok(())
    }

    pub fn clear_wallpaper(&self, _monitor_id: String) -> Result<()> {
        Ok(())
    }

    pub fn set_paused(&self, _paused: bool) -> Result<()> {
        Ok(())
    }

    pub fn shutdown(&mut self) {}
}
