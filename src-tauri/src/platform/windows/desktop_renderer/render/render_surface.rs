//! 单屏壁纸渲染状态与绘制。
//!
//! 自有窗口 / swapchain / RTV，但共享 device / pipeline / video worker。
//! 壁纸内容分三层：`wallpaper`（fade 期间仍是 from）、`transition`（to）、`pending`。

use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result};
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11DepthStencilView, ID3D11Device, ID3D11DeviceContext, ID3D11RenderTargetView,
    ID3D11Texture2D, D3D11_VIEWPORT,
};
use windows::Win32::Graphics::DirectComposition::{
    IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::{IDXGISwapChain1, DXGI_PRESENT, DXGI_SWAP_CHAIN_FLAG};

use super::composition::create_composition_swap_chain;
use super::pipeline::{FadeConstants, QuadPipeline, Vertex};
use super::shared_device::SharedDevice;
use crate::platform::wallpaper_renderer::fit::{fit_rect, FitRect};
use crate::platform::wallpaper_renderer::{
    crossfade_progress, ExtendViewport, FitMode, MonitorRect, CROSSFADE_DURATION,
};
use crate::platform::windows::desktop_embedder::EmbedPolicy;
use crate::platform::windows::desktop_renderer::decode::SharedFrames;
use crate::platform::windows::desktop_renderer::decode::{VideoFrameSnapshot, VideoPlayback};
use crate::platform::windows::desktop_renderer::device::{is_device_lost, NativeWindow};

#[derive(Clone)]
pub enum WallpaperMedia {
    Frames(Arc<SharedFrames>),
    /// Mutex 只用于让 renderer loop 在绘制前推进时钟；所有访问都在同一渲染线程，
    /// 实际不会产生锁竞争，但避免用 unsafe 把 Arc 转成 &mut。
    Video(Arc<Mutex<VideoPlayback>>),
}

/// 正在进行的淡入淡出：`wallpaper` 仍是 from，to 单独持有。
///
/// 不把 to 直接挂到 `wallpaper`：transition 期间 renderer loop 必须能区分
/// from / to 并冻结两侧视频时钟，结束后才把 to 提升为当前壁纸。
struct Transition {
    // to 单独挂在 transition 而非直接覆盖 wallpaper：fade 期间 wallpaper 仍是 from，
    // renderer loop 才能同时冻结两侧视频时钟，并在结束时只提升一次 to。
    to: WallpaperMedia,
    to_fit_mode: FitMode,
    // fade 时间线起点；组内所有 surface 传入同一个 now，避免接缝明暗不同步。
    started_at: Instant,
}

/// 排队等待的换图。连续切换不覆盖旧 fade：A→B 跑完才启动 B→C。
struct PendingWallpaper {
    media: WallpaperMedia,
    fit_mode: FitMode,
}

/// 渲染线程独有的共享 D3D11 上下文。
///
/// SRV 不能跨 device 使用，而 extend / mirror 组内所有 surface 必须绑定同一份纹理，
/// 因此 device / context / pipeline 全局只能有一份。所有 surface 都在同一渲染线程上
/// 绘制，即时上下文的非线程安全约束自动满足。

pub struct RenderSurface {
    window: NativeWindow,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    // flip model 的 backbuffer 容器；每次 Present 后轮转，需重新取 GetBuffer(0) 建 RTV。
    swap_chain: IDXGISwapChain1,
    // DirectComposition COM 对象必须活到 RenderSurface 析构；字段只用于持有引用，不直接读取。
    #[allow(dead_code)]
    composition_device: IDCompositionDevice,
    #[allow(dead_code)]
    composition_target: IDCompositionTarget,
    #[allow(dead_code)]
    composition_visual: IDCompositionVisual,
    // 绑定当前 backbuffer 的 render target view；None 表示尚未绑定 / 已释放。
    render_target: Option<ID3D11RenderTargetView>,
    // 来自共享层：extend 组内所有 surface 必须绑定同一份 SRV，故不可各自新建。
    pipeline: QuadPipeline,
    rect: MonitorRect,
    fit_mode: FitMode,
    /// Some 表示该屏参与 extend 拼接；None 表示按本屏尺寸独立 fit。
    extend_viewport: Option<ExtendViewport>,
    // 是否仍嵌入桌面；destroy 据此决定是否 unembed，避免重复调用。
    embedded: bool,
    // 当前显示内容；fade 期间这里仍是 from，to 放在 transition 里。
    wallpaper: Option<WallpaperMedia>,
    // 正在进行的淡入淡出；视频时钟冻结与 fade 推进都以此为准。
    transition: Option<Transition>,
    // 排队换图：连续切换只保留最新一张，避免每个视频各自持有 GPU 导致内存暴涨。
    pending: VecDeque<PendingWallpaper>,
    // Present / backbuffer 错误只记录一次，避免每帧刷屏
    // 成功后复位，以便下次出错时还能再报一次。
    present_error_logged: bool,
    backbuffer_error_logged: bool,
}

impl RenderSurface {
    /// 创建并嵌入壁纸窗口，再挂 swapchain / DirectComposition / RTV；
    /// 首帧前先 Present 一张全透明 backbuffer，避免默认黑色闪一下。
    pub fn new(shared: &SharedDevice, rect: MonitorRect) -> Result<Self> {
        let window = NativeWindow::new(rect)?;
        let hwnd = window.hwnd();

        // DirectComposition target 必须绑定嵌入后的最终 HWND，且窗口创建期就要带
        // WS_EX_NOREDIRECTIONBITMAP；否则 HWND bitblt swapchain 不会被 24H2 桌面合成树采样。
        crate::platform::windows::desktop_embedder::embed_in_desktop(
            hwnd.0 as isize,
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            EmbedPolicy::Native,
        )
        .context("原生壁纸窗口嵌入桌面失败")?;
        window.show();
        window.normalize_native_ex_style()?;

        // 每屏仍要有自己的 swapchain / target，但 device 与 pipeline 必须来自共享层：
        // extend 组内所有 surface 要绑定同一份 SRV。
        let composition = create_composition_swap_chain(
            &shared.device,
            hwnd,
            rect.width as u32,
            rect.height as u32,
        )?;

        let mut surface = Self {
            window,
            device: shared.device.clone(),
            context: shared.context.clone(),
            swap_chain: composition.swap_chain,
            composition_device: composition.device,
            composition_target: composition.target,
            composition_visual: composition.visual,
            render_target: None,
            pipeline: shared.pipeline.clone(),
            rect,
            fit_mode: FitMode::Cover,
            extend_viewport: None,
            embedded: true,
            wallpaper: None,
            transition: None,
            pending: VecDeque::new(),
            present_error_logged: false,
            backbuffer_error_logged: false,
        };
        surface.bind_current_backbuffer()?;
        // 首帧前先提交一张完全透明的 backbuffer，避免 DirectComposition 初始内容
        // 在 renderer loop 第一帧之前显示为黑色。
        if let Some(render_target) = &surface.render_target {
            unsafe {
                surface
                    .context
                    .ClearRenderTargetView(render_target, &[0.0, 0.0, 0.0, 0.0]);
                surface.swap_chain.Present(1, DXGI_PRESENT(0)).ok()?;
                surface.composition_device.Commit()?;
            }
        }
        Ok(surface)
    }

    /// 挂上一张新壁纸。`crossfade = false`（跨 backend / 首帧）直接替换并丢弃队列；
    /// `true` 时保留当前 fade，但排队项只保留最新一张——每个视频都持有独立
    /// DXVA worker / D3D11 device / SourceReader，FIFO 堆积会让连续切换暴涨内存。
    pub fn queue_wallpaper(
        &mut self,
        media: WallpaperMedia,
        fit_mode: FitMode,
        crossfade: bool,
        now: Instant,
    ) {
        if !crossfade || self.wallpaper.is_none() {
            log::info!(
                "[Renderer] wallpaper 硬切换 crossfade={} pending_before={} transition_before={}",
                crossfade,
                self.pending.len(),
                self.transition.is_some()
            );
            self.transition = None;
            self.pending.clear();
            self.wallpaper = Some(media);
            self.fit_mode = fit_mode;
            return;
        }
        // 只保留最新排队项；被丢弃的 Arc 会立即触发 VideoPlayback::Drop，
        // worker 收到 Exit 后释放 SourceReader / device manager / shared texture。
        log::info!(
            "[Renderer] wallpaper 排队 pending_before={} transition_active={}",
            self.pending.len(),
            self.transition.is_some()
        );
        self.pending.clear();
        self.pending.push_back(PendingWallpaper { media, fit_mode });
        if self.transition.is_none() {
            self.start_next_transition(now);
        }
    }

    /// 从队首取出下一张开启 fade；队列为空则保持无 transition。
    /// 沿用同一个 now 续接上一段，使组内所有 surface 的 fade 时间线一致。
    fn start_next_transition(&mut self, now: Instant) {
        let Some(next) = self.pending.pop_front() else {
            return;
        };
        self.transition = Some(Transition {
            to: next.media,
            to_fit_mode: next.fit_mode,
            started_at: now,
        });
    }

    /// fade 结束：把 to 提升为当前壁纸，返回是否刚结束一段。
    /// 视频时钟的重置由 renderer loop 按 Arc 指针去重执行——组内多屏共享同一个
    /// `VideoPlayback`，在每块屏上各 reset 会重复 Flush / seek 同一个 SourceReader。
    pub fn finish_transition(&mut self, now: Instant) -> bool {
        let done = self
            .transition
            .as_ref()
            .map(|t| now.saturating_duration_since(t.started_at) >= CROSSFADE_DURATION)
            .unwrap_or(false);
        if !done {
            return false;
        }
        let Some(transition) = self.transition.take() else {
            return false;
        };
        self.wallpaper = Some(transition.to);
        self.fit_mode = transition.to_fit_mode;
        // 用同一个 now 续接下一段，组内所有 surface 的 fade 时间线保持一致。
        self.start_next_transition(now);
        true
    }

    /// 是否处于 fade 中或还有排队项。renderer loop 据此冻结 from / to 的视频时钟。
    pub fn fade_active_or_pending(&self) -> bool {
        self.transition.is_some() || !self.pending.is_empty()
    }

    /// 该屏是否需要主循环按 16ms 持续出帧。纯静态单帧图不需要，
    /// 视频 / fade / 动图都需要按帧时钟推进，否则会出现跳帧或 fade 卡顿。
    pub fn needs_continuous_frames(&self) -> bool {
        if self.fade_active_or_pending() || !self.video_arcs().is_empty() {
            return true;
        }
        matches!(&self.wallpaper, Some(WallpaperMedia::Frames(frames)) if frames.is_animated())
    }

    /// 当前壁纸（fade 期间即 from）的视频播放状态；由 renderer loop 在绘制整组前统一推进。
    pub fn video(&self) -> Option<&Arc<Mutex<VideoPlayback>>> {
        match &self.wallpaper {
            Some(WallpaperMedia::Video(playback)) => Some(playback),
            _ => None,
        }
    }

    /// 当前 / 过渡中 / 排队中的所有视频播放状态。
    /// 音量与 fade 冻结必须覆盖 to 与队列里的视频，否则新音频会在 fade 前提前出声。
    pub fn video_arcs(&self) -> Vec<&Arc<Mutex<VideoPlayback>>> {
        let mut arcs = Vec::new();
        if let Some(playback) = self.video() {
            arcs.push(playback);
        }
        if let Some(transition) = &self.transition {
            if let WallpaperMedia::Video(playback) = &transition.to {
                arcs.push(playback);
            }
        }
        for pending in &self.pending {
            if let WallpaperMedia::Video(playback) = &pending.media {
                arcs.push(playback);
            }
        }
        arcs
    }

    /// 清空三层内容，视频随之停止出声；窗口与 RTV 保留以便复用。
    pub fn clear_wallpaper(&mut self) {
        self.wallpaper = None;
        self.transition = None;
        self.pending.clear();
    }

    /// 更新 extend 裁剪参数；None 表示该屏退出拼接、退回独立 fit。
    pub fn set_extend_viewport(&mut self, viewport: Option<ExtendViewport>) {
        self.extend_viewport = viewport;
    }

    /// 单屏 fit 模式；extend 生效时由 viewport 决定裁剪，该值被忽略。
    pub fn set_fit_mode(&mut self, fit_mode: FitMode) {
        self.fit_mode = fit_mode;
    }

    /// 显示器几何变化时同步原生窗口、swapchain 与 render target，不重建整张 surface。
    pub fn update_rect(&mut self, rect: MonitorRect) -> Result<()> {
        if self.rect == rect {
            return Ok(());
        }

        // ResizeBuffers 要求先释放所有 backbuffer 引用，否则返回 DXGI_ERROR_INVALID_CALL
        unsafe {
            self.context
                .OMSetRenderTargets(Some(&[None]), None::<&ID3D11DepthStencilView>);
        }
        self.render_target = None;

        self.window.set_bounds(rect)?;
        unsafe {
            self.swap_chain.ResizeBuffers(
                2,
                rect.width as u32,
                rect.height as u32,
                DXGI_FORMAT_B8G8R8A8_UNORM,
                DXGI_SWAP_CHAIN_FLAG(0),
            )?;
        }

        self.rect = rect;
        self.bind_current_backbuffer()
    }

    /// Flip model 的 backbuffer 会在每次 Present 后轮转，必须重新取 GetBuffer(0) 并绑定 RTV。
    fn bind_current_backbuffer(&mut self) -> Result<()> {
        unsafe {
            self.context
                .OMSetRenderTargets(Some(&[None]), None::<&ID3D11DepthStencilView>);
        }
        self.render_target = None;

        let back_buffer: ID3D11Texture2D = unsafe { self.swap_chain.GetBuffer(0)? };
        let mut render_target: Option<ID3D11RenderTargetView> = None;
        unsafe {
            self.device
                .CreateRenderTargetView(&back_buffer, None, Some(&mut render_target))?;
        }
        let render_target = render_target.context("D3D11 render target 创建失败")?;
        unsafe {
            self.context.OMSetRenderTargets(
                Some(&[Some(render_target.clone())]),
                None::<&ID3D11DepthStencilView>,
            );
        }

        self.render_target = Some(render_target);
        Ok(())
    }

    /// 帧是否可绘制：静态帧恒为 true；视频需等首帧到达，避免采样全 0 NV12 出现绿屏。
    fn media_has_drawable_frame(media: &WallpaperMedia) -> bool {
        match media {
            WallpaperMedia::Frames(_) => true,
            WallpaperMedia::Video(playback) => playback
                .lock()
                .map(|guard| guard.is_frame_ready())
                .unwrap_or(false),
        }
    }

    /// fade 期间 from / to 任一有帧即可绘制；否则只有当前壁纸有帧才绘制。
    fn has_drawable_frame(&self) -> bool {
        if let Some(transition) = &self.transition {
            self.wallpaper
                .as_ref()
                .is_some_and(Self::media_has_drawable_frame)
                || Self::media_has_drawable_frame(&transition.to)
        } else {
            self.wallpaper
                .as_ref()
                .is_some_and(Self::media_has_drawable_frame)
        }
    }

    /// 一帧渲染：重绑 backbuffer → 清屏 → 绘制 from / to（或单层壁纸）→ Present。
    /// backbuffer / Present 失败各只记一次日志；透明清屏用于让系统默认壁纸透出。
    ///
    /// 返回 true 表示检测到设备丢失（DXGI_ERROR_DEVICE_REMOVED / DEVICE_RESET）：
    /// 该 surface 的 D3D11 资源已整体失效，调用方必须清理整组 surface 与共享 device，
    /// 不能继续在这块 device 上 Present。
    pub fn render(&mut self, monitor_id: &str, now: Instant) -> bool {
        if let Err(err) = self.bind_current_backbuffer() {
            if is_device_lost(&err) {
                log::error!(
                    "[Renderer] {} backbuffer 绑定时设备丢失: {:#}",
                    monitor_id,
                    err
                );
                return true;
            }
            if !self.backbuffer_error_logged {
                log::error!(
                    "[Renderer] {} 当前 backbuffer 绑定失败: {:#}",
                    monitor_id,
                    err
                );
                self.backbuffer_error_logged = true;
            }
            return false;
        }
        self.backbuffer_error_logged = false;

        let Some(render_target) = &self.render_target else {
            return false;
        };
        let has_wallpaper = self.wallpaper.is_some() || self.transition.is_some();

        // 有壁纸但还没有任何可绘制帧（典型：视频首帧未到）时清成完全透明，
        // 让系统默认壁纸透出；swapchain 已声明 premultiplied alpha。
        let clear_color = if has_wallpaper && !self.has_drawable_frame() {
            [0.0, 0.0, 0.0, 0.0]
        } else {
            [0.08, 0.12, 0.18, 1.0]
        };
        unsafe {
            self.context
                .ClearRenderTargetView(render_target, &clear_color);
        }

        if let Some(transition) = &self.transition {
            // crossfade_progress = 已过时长 / CROSSFADE_DURATION，作为 to 的混合权重（0→1）。
            let progress = crossfade_progress(
                now.saturating_duration_since(transition.started_at),
                CROSSFADE_DURATION,
            );
            // from 先以 alpha=1 铺满 RT，to 再用 progress 混合一次即可。
            // 两侧都带 alpha 会让 from 被衰减两次，出现中段发暗。
            if let Some(from) = &self.wallpaper {
                self.draw_media(from, self.fit_mode, 1.0, now);
            }
            self.draw_media(&transition.to, transition.to_fit_mode, progress, now);
        } else if let Some(wallpaper) = &self.wallpaper {
            self.draw_media(wallpaper, self.fit_mode, 1.0, now);
        }

        let present = unsafe { self.swap_chain.Present(1, DXGI_PRESENT(0)) };
        if let Err(err) = present.ok() {
            if is_device_lost(&err) {
                log::error!("[Renderer] {} Present 时设备丢失: {:#}", monitor_id, err);
                return true;
            }
            if !self.present_error_logged {
                log::warn!("[Renderer] {} Present 失败: {:?}", monitor_id, err);
                self.present_error_logged = true;
            }
        } else {
            // 与 backbuffer_error_logged 对称：成功后复位，下次再出错才能再报一次。
            self.present_error_logged = false;
        }
        false
    }

    /// 一次全屏 quad 绘制。帧来源、fit / extend 视口推导在两条媒体链路间完全共用；
    /// fade 只改变写入 cbuffer 的 alpha，因此不会引入第二套绘制路径。
    fn draw_media(&self, media: &WallpaperMedia, fit_mode: FitMode, alpha: f32, now: Instant) {
        // 先取出 (SRV, 宽, 高)，两条媒体链路在这一步之后完全共用同一套 quad / viewport。
        // VideoPlayback 的时钟由 renderer loop 在绘制整组前统一推进，这里只读当前帧。
        let current: Option<VideoFrameSnapshot> = match media {
            WallpaperMedia::Frames(frames) => {
                let frame = frames.current_frame(now);
                Some(VideoFrameSnapshot::for_static_frame(
                    frame.view.clone(),
                    frame.width,
                    frame.height,
                ))
            }
            WallpaperMedia::Video(playback) => playback
                .lock()
                .ok()
                .and_then(|guard| guard.frame_snapshot()),
        };
        let Some(current) = current else {
            return;
        };
        let view = current.view.clone();
        let uv_view = current.uv_view.clone();
        let frame_width = current.width;
        let frame_height = current.height;
        let yuv = current.yuv;

        let pipeline = &self.pipeline;
        let vertex_buffers = [Some(pipeline.vertex_buffer.clone())];
        let strides = [std::mem::size_of::<Vertex>() as u32];
        let offsets = [0u32];
        let samplers = [Some(pipeline.sampler.clone())];
        let views = [Some(view), uv_view];
        let pixel_shader = if views[1].is_some() {
            &pipeline.nv12_pixel_shader
        } else {
            &pipeline.pixel_shader
        };
        let constant_buffers = [Some(pipeline.fade_buffer.clone())];
        // Q1：extend 与 WebView 的 object-fit:fill 语义一致——忽略 fitMode，
        // 整张图拉伸铺满所有显示器的并集，再用负 TopLeftX/Y 把本屏那块裁进 RT。
        // 负 viewport 允许存在，超出 RT 的部分由光栅化裁掉，quad / shader 不用改。
        let (target_w, target_h, fit, offset_x, offset_y) = match self.extend_viewport {
            Some(v) => (
                v.total_width as u32,
                v.total_height as u32,
                FitMode::Fill,
                v.offset_x as f32,
                v.offset_y as f32,
            ),
            None => (
                self.rect.width as u32,
                self.rect.height as u32,
                fit_mode,
                0.0,
                0.0,
            ),
        };
        let FitRect {
            x,
            y,
            width,
            height,
        } = fit_rect(fit, frame_width, frame_height, target_w, target_h);
        let viewport = D3D11_VIEWPORT {
            TopLeftX: x - offset_x,
            TopLeftY: y - offset_y,
            Width: width,
            Height: height,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        let fade = FadeConstants {
            alpha,
            _pad0: [0.0; 3],
            yuv0: [yuv.y_offset, yuv.y_scale, yuv.r_v, yuv.g_u],
            yuv1: [yuv.g_v, yuv.b_u, 0.0, 0.0],
        };

        unsafe {
            self.context
                .OMSetBlendState(&pipeline.blend_state, None, u32::MAX);
            self.context.UpdateSubresource(
                &pipeline.fade_buffer,
                0,
                None,
                &fade as *const FadeConstants as *const c_void,
                0,
                0,
            );
            self.context
                .PSSetConstantBuffers(0, Some(&constant_buffers));
            self.context.IASetInputLayout(&pipeline.input_layout);
            self.context.IASetVertexBuffers(
                0,
                1,
                Some(vertex_buffers.as_ptr()),
                Some(strides.as_ptr()),
                Some(offsets.as_ptr()),
            );
            self.context
                .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            self.context.VSSetShader(&pipeline.vertex_shader, None);
            self.context.PSSetShader(pixel_shader, None);
            self.context.PSSetSamplers(0, Some(&samplers));
            self.context.PSSetShaderResources(0, Some(&views));
            // QUAD 顶点顺序与 D3D 默认正面绕序相反，不显式关背面剔除会丢掉全部三角形
            self.context.RSSetState(&pipeline.rasterizer_state);
            self.context.RSSetViewports(Some(&[viewport]));
            self.context.Draw(4, 0);
            // 视频帧来自 renderer 本地副本；keyed mutex 已在帧开始时统一释放。
        }
    }

    /// 释放三层壁纸内容并按需从桌面取消嵌入；窗口 / 设备交给各自 Drop 回收。
    pub fn destroy(&mut self) {
        // 先解绑 context 对 RTV 的引用，再清字段；与 bind_current_backbuffer 开头
        // 的写法一致，让 destroy 的释放语义显式，不依赖 COM 引用计数延迟回收。
        unsafe {
            self.context
                .OMSetRenderTargets(Some(&[None]), None::<&ID3D11DepthStencilView>);
        }
        self.render_target = None;
        self.wallpaper = None;
        self.transition = None;
        self.pending.clear();
        if self.embedded {
            crate::platform::windows::desktop_embedder::unembed_from_desktop(
                self.window.hwnd().0 as isize,
            );
            self.embedded = false;
        }
    }
}

impl Drop for RenderSurface {
    fn drop(&mut self) {
        self.destroy();
    }
}
