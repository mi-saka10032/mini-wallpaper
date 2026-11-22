#![allow(dead_code, unused_imports)]

use std::path::PathBuf;

use anyhow::Result;

// 跨模块共享的契约类型与错误定义（对外通过下方 pub use 暴露）
mod contracts;
// CPU 侧素材解码：GIF / WebP / 静态图 → BGRA 帧序列
pub mod decoder;
// fit 模式到目标矩形的几何映射
pub mod fit;
// 与 GPU 无关的 key / LRU / 字节预算缓存逻辑
pub mod frame_cache;
// 跨屏并集与 extend 裁剪视口计算
pub mod geometry;
// 后端选择策略（Native / WebView 回退）
mod selection;

// 统一对外导出契约类型，让上层无需感知内部分模块结构。
pub use contracts::{
    crossfade_progress, BackendSelection, DisplayMode, ExtendViewport, FitMode, MonitorRect,
    RenderBackend, RenderDescriptor, RendererError, RendererErrorCode, WallpaperKind,
    CROSSFADE_DURATION,
};
// 跨屏几何与后端决策的对外入口，供上层协调器直接调用。
pub use geometry::{extend_viewport, group_viewports, union_rect};
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
    /// 占位：非 Windows 平台不启动渲染线程，仅返回空实例以保证跨平台编译通过。
    pub fn start() -> Result<Self> {
        Ok(Self)
    }

    /// 占位：真实实现会为该显示器创建或复用渲染目标。
    pub async fn ensure_monitor(&self, _monitor_id: String, _rect: MonitorRect) -> Result<()> {
        Ok(())
    }

    /// 占位：真实实现会释放该显示器对应的渲染目标。
    pub fn destroy_monitor(&self, _monitor_id: String) -> Result<()> {
        Ok(())
    }

    /// 一组显示器共享同一次解码与同一个帧时钟；independent 时组内只有一个 id。
    pub async fn set_wallpaper(
        &self,
        _monitor_ids: Vec<String>,
        _path: PathBuf,
        _kind: WallpaperKind,
        _fit_mode: FitMode,
        _target_width: u32,
        _target_height: u32,
        _crossfade: bool,
    ) -> Result<()> {
        Ok(())
    }

    /// 全组更新 extend 裁剪参数；None 表示该屏退回各自渲染（Q1 / D2）。
    pub fn set_group_geometry(
        &self,
        _viewports: Vec<(String, Option<ExtendViewport>)>,
    ) -> Result<()> {
        Ok(())
    }

    /// 占位：真实实现会更新某屏的缩放/裁剪策略。
    pub fn set_fit_mode(&self, _monitor_id: String, _fit_mode: FitMode) -> Result<()> {
        Ok(())
    }

    /// 占位：真实实现会清除某屏壁纸（通常是以淡出或直接清屏收尾）。
    pub fn clear_wallpaper(&self, _monitor_id: String) -> Result<()> {
        Ok(())
    }

    /// 占位：真实实现会暂停/恢复 GIF、视频的帧时钟。
    pub fn set_paused(&self, _paused: bool) -> Result<()> {
        Ok(())
    }

    /// 占位：真实实现会调整视频播放音量。
    pub fn set_volume(&self, _volume: u32) -> Result<()> {
        Ok(())
    }

    /// 占位：真实实现会停止渲染线程并回收资源。
    pub fn shutdown(&mut self) {}
}
