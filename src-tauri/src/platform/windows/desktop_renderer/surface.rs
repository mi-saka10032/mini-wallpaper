use std::collections::VecDeque;
use std::ffi::c_void;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use windows::core::{IUnknown, Interface, BOOL, PCSTR};
use windows::Win32::Foundation::{FALSE, HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{
    ID3DBlob, ID3DInclude, D3D_DRIVER_TYPE_HARDWARE, D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11BlendState, ID3D11Buffer, ID3D11DepthStencilView, ID3D11Device,
    ID3D11DeviceContext, ID3D11InputLayout, ID3D11PixelShader, ID3D11RasterizerState,
    ID3D11RenderTargetView, ID3D11SamplerState, ID3D11ShaderResourceView, ID3D11Texture2D,
    ID3D11VertexShader, D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_VERTEX_BUFFER, D3D11_BLEND_DESC,
    D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ONE, D3D11_BLEND_OP_ADD, D3D11_BUFFER_DESC,
    D3D11_COLOR_WRITE_ENABLE_ALL, D3D11_COMPARISON_ALWAYS, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_CULL_NONE, D3D11_FILL_SOLID, D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_INPUT_ELEMENT_DESC,
    D3D11_INPUT_PER_VERTEX_DATA, D3D11_RASTERIZER_DESC, D3D11_RENDER_TARGET_BLEND_DESC,
    D3D11_SAMPLER_DESC, D3D11_SDK_VERSION, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE_ADDRESS_CLAMP,
    D3D11_USAGE_DEFAULT, D3D11_VIEWPORT,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R32G32_FLOAT,
    DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGIAdapter, IDXGIDevice, IDXGIFactory2, IDXGIOutput, IDXGISwapChain1, DXGI_PRESENT,
    DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};

use super::video::{VideoFrameSnapshot, VideoPlayback, VideoWorkerHost};
use super::window::NativeWindow;
use crate::platform::wallpaper_renderer::fit::{fit_rect, FitRect};
use crate::platform::wallpaper_renderer::frame_cache::ByteSize;
use crate::platform::wallpaper_renderer::{
    crossfade_progress, ExtendViewport, FitMode, MonitorRect, CROSSFADE_DURATION,
};
use crate::platform::windows::desktop_embedder::EmbedPolicy;

/// 全屏 quad 的 HLSL：图片走 B8G8R8A8 采样，视频走 NV12 双平面采样；
/// 两者共用同一个 fade_alpha cbuffer，因此 fade 不需要第二套绘制路径。
const SHADER_SOURCE: &str = r#"
struct VS_IN {
    float2 pos : POSITION;
    float2 uv : TEXCOORD0;
};

struct VS_OUT {
    float4 pos : SV_POSITION;
    float2 uv : TEXCOORD0;
};

VS_OUT vs_main(VS_IN input) {
    VS_OUT output;
    output.pos = float4(input.pos, 0.0, 1.0);
    output.uv = input.uv;
    return output;
}

Texture2D wallpaper : register(t0);
SamplerState linear_sampler : register(s0);

cbuffer Fade : register(b0) {
    float fade_alpha;
    float3 _pad;
};

float4 ps_main(VS_OUT input) : SV_TARGET {
    float4 color = wallpaper.Sample(linear_sampler, input.uv);
    // swapchain 声明为 premultiplied alpha，输出必须预乘；否则两 pass 混合会
    // 把 RT 的 alpha 拉到 <1，桌面合成阶段整屏变暗。
    color.rgb *= fade_alpha;
    color.a = fade_alpha;
    return color;
}

Texture2D y_plane : register(t0);
Texture2D uv_plane : register(t1);

float4 ps_nv12(VS_OUT input) : SV_TARGET {
    // BT.601 limited-range NV12 → RGB，全部在 GPU 侧完成，避免 CPU 逐像素转换。
    float y = saturate((y_plane.Sample(linear_sampler, input.uv).r - 16.0 / 255.0) * 1.164383);
    float2 uv = uv_plane.Sample(linear_sampler, input.uv).rg - 0.5;
    float r = saturate(y + 1.596027 * uv.y);
    float g = saturate(y - 0.391762 * uv.x - 0.812968 * uv.y);
    float b = saturate(y + 2.017232 * uv.x);
    return float4(float3(r, g, b) * fade_alpha, fade_alpha);
}
"#;

/// 与 input layout（POSITION / TEXCOORD）一一对应的全屏 quad 顶点。
#[repr(C)]
struct Vertex {
    position: [f32; 2],
    texcoord: [f32; 2],
}

/// cbuffer 最小分配粒度是 16 bytes，单 float 必须补齐。
#[repr(C)]
struct FadeConstants {
    alpha: f32,
    pad: [f32; 3],
}

const QUAD: [Vertex; 4] = [
    Vertex {
        position: [-1.0, -1.0],
        texcoord: [0.0, 1.0],
    },
    Vertex {
        position: [1.0, -1.0],
        texcoord: [1.0, 1.0],
    },
    Vertex {
        position: [-1.0, 1.0],
        texcoord: [0.0, 0.0],
    },
    Vertex {
        position: [1.0, 1.0],
        texcoord: [1.0, 0.0],
    },
];

/// 全屏 quad 所需的对象集合：由 SharedDevice 创建一次并克隆给所有 surface。
/// blend state + fade cbuffer 承载 crossfade 的混合数学，nv12 pixel shader 专供视频链路。
#[derive(Clone)]
struct QuadPipeline {
    vertex_shader: ID3D11VertexShader,
    pixel_shader: ID3D11PixelShader,
    nv12_pixel_shader: ID3D11PixelShader,
    rasterizer_state: ID3D11RasterizerState,
    input_layout: ID3D11InputLayout,
    vertex_buffer: ID3D11Buffer,
    sampler: ID3D11SamplerState,
    blend_state: ID3D11BlendState,
    fade_buffer: ID3D11Buffer,
}

/// swapchain 经 DirectComposition visual 挂到嵌入后的壁纸 HWND。
/// 这几个 COM 对象必须一起存活，否则 visual 失效、桌面合成树取不到内容。
struct CompositionSwapChain {
    swap_chain: IDXGISwapChain1,
    device: IDCompositionDevice,
    target: IDCompositionTarget,
    visual: IDCompositionVisual,
}

/// 一张已上传 GPU 的帧：`view` 是 SRV（内部持有纹理引用），duration 供动图取模选帧。
pub(crate) struct GpuFrame {
    pub(crate) view: ID3D11ShaderResourceView,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) duration: Duration,
}

/// 一组显示器共享的帧序列与帧时钟。
///
/// extend / mirror 组内所有 surface 持有同一个 Arc，天然选到同一帧；每屏各自
/// `Instant::now()` 会让跨屏接缝出现不同帧，正是 WebView 侧 useVideoSync 想解决的问题。
pub struct SharedFrames {
    frames: Vec<GpuFrame>,
    // 组内所有 surface 共用这个起点，保证跨屏选到同一帧。
    started_at: Instant,
}

impl SharedFrames {
    /// 由 image worker 组装帧序列；帧时钟起点在 attach 到 surface 时才由
    /// renderer 线程通过 `reset_clock` 对齐，这里给一个占位起点。
    pub(crate) fn new(frames: Vec<GpuFrame>) -> Self {
        Self {
            frames,
            started_at: Instant::now(),
        }
    }

    /// 把帧时钟起点对齐到真正 attach 的时刻，保证 GIF 从第一帧开始、
    /// 组内所有屏共享同一条时间线。
    pub(crate) fn reset_clock(&mut self, started_at: Instant) {
        self.started_at = started_at;
    }

    /// 多帧动图需要按帧时钟持续重绘；单帧静态图不需要。
    pub(crate) fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }

    fn current_frame(&self, now: Instant) -> &GpuFrame {
        if self.frames.len() <= 1 {
            return &self.frames[0];
        }

        // 按总时长取模，让 GIF / 动态 WebP 循环播放而不是播完一轮停在首帧
        let total: Duration = self.frames.iter().map(|frame| frame.duration).sum();
        if total.is_zero() {
            return &self.frames[0];
        }
        let elapsed = now.saturating_duration_since(self.started_at).as_nanos() % total.as_nanos();

        let mut accumulated = 0u128;
        for frame in &self.frames {
            accumulated += frame.duration.as_nanos();
            if elapsed < accumulated {
                return frame;
            }
        }

        &self.frames[0]
    }
}

/// 缓存按显存字节数淘汰，多帧 GIF / 高分辨率素材会成倍占用；
/// 这里只统计 GPU 纹理的原始像素量，不含驱动侧的对齐与内部副本。
impl ByteSize for SharedFrames {
    fn byte_size(&self) -> usize {
        self.frames
            .iter()
            .map(|frame| frame.width as usize * frame.height as usize * 4)
            .sum()
    }
}

/// 一张壁纸在 surface 上的两种来源。
///
/// 图片 / GIF / 动态 WebP 用预解码的 `SharedFrames`（按 started_at 取模选帧）；
/// 视频用流式 `VideoPlayback`（每帧由 renderer loop 推进一次）。两条链路共用
/// 同一个 device / pipeline 与全屏 quad，但帧时钟机制不同，故用枚举分开。
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
pub struct SharedDevice {
    device: ID3D11Device,
    // 即时上下文非线程安全；所有 surface 都在同一渲染线程上用它绘制。
    context: ID3D11DeviceContext,
    pipeline: QuadPipeline,
    // 单个 video worker 服务全部 VideoPlayback；每路播放自带独立 GPU / SourceReader。
    video_worker: VideoWorkerHost,
}

impl SharedDevice {
    /// 暴露 device 供 image worker 在渲染线程之外上传纹理；SRV 不能跨 device 使用，
    /// 故 worker 必须克隆这同一个 device，不能自建。
    pub(crate) fn device(&self) -> &ID3D11Device {
        &self.device
    }

    pub fn new() -> Result<Self> {
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        unsafe {
            D3D11CreateDevice(
                None::<&IDXGIAdapter>,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        let device = device.context("D3D11 device 创建失败")?;
        let context = context.context("D3D11 context 创建失败")?;
        let pipeline = create_pipeline(&device)?;
        let video_worker = VideoWorkerHost::start(&device)?;
        Ok(Self {
            device,
            context,
            pipeline,
            video_worker,
        })
    }

    /// 打开一个流式视频播放状态；复用共享 device / context，不进 FrameCache
    /// （视频不缓存全部帧，缓存的是唯一一个正在推进的播放状态）。
    pub fn open_video(&self, path: &Path, volume: u32) -> Result<Arc<Mutex<VideoPlayback>>> {
        let playback = VideoPlayback::open(
            &self.device,
            &self.context,
            &self.video_worker,
            path,
            volume,
        )?;
        Ok(Arc::new(Mutex::new(playback)))
    }
}

/// 单屏壁纸渲染状态：自有窗口 / swapchain / RTV，但共享 device / pipeline / video worker。
///
/// 壁纸内容分三层保存：`wallpaper`（当前，fade 期间即 from）、`transition`（fade 的 to）、
/// `pending`（排队换图）。视频的暂停与音量必须覆盖全部三层，见 `video_arcs()`。
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

    /// 更新 extend 裁剪参数；None 表示该屏退出拼接、退回独立 fit。
    pub fn set_extend_viewport(&mut self, viewport: Option<ExtendViewport>) {
        self.extend_viewport = viewport;
    }

    /// 单屏 fit 模式；extend 生效时由 viewport 决定裁剪，该值被忽略。
    pub fn set_fit_mode(&mut self, fit_mode: FitMode) {
        self.fit_mode = fit_mode;
    }

    /// 清空三层内容，视频随之停止出声；窗口与 RTV 保留以便复用。
    pub fn clear_wallpaper(&mut self) {
        self.wallpaper = None;
        self.transition = None;
        self.pending.clear();
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
            pad: [0.0; 3],
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

/// 从两类错误中提取 HRESULT：bind_current_backbuffer 经 anyhow 包装（可能多层 Context），
/// Present 直接返回 windows 原生错误。统一成一个 trait，让 is_device_lost 泛型处理两者。
trait DeviceLostHResult {
    fn lost_hresult(&self) -> Option<windows::core::HRESULT>;
}

impl DeviceLostHResult for anyhow::Error {
    fn lost_hresult(&self) -> Option<windows::core::HRESULT> {
        // 沿 anyhow 错误链找 windows::core::Error，避免依赖具体被哪层 Context 包装。
        self.chain().find_map(|cause| {
            cause
                .downcast_ref::<windows::core::Error>()
                .map(|err| err.code())
        })
    }
}

impl DeviceLostHResult for windows::core::Error {
    fn lost_hresult(&self) -> Option<windows::core::HRESULT> {
        Some(self.code())
    }
}

/// 判断 D3D11 / DXGI 错误是否属于设备丢失。设备丢失时整块 device 上的所有资源
/// 同时失效，必须整组清理 surface 与 SharedDevice，不能只重试单屏。
fn is_device_lost<E: DeviceLostHResult>(err: &E) -> bool {
    err.lost_hresult().is_some_and(|code| {
        code == windows::Win32::Graphics::Dxgi::DXGI_ERROR_DEVICE_REMOVED
            || code == windows::Win32::Graphics::Dxgi::DXGI_ERROR_DEVICE_RESET
            || code == windows::Win32::Graphics::Dxgi::DXGI_ERROR_DRIVER_INTERNAL_ERROR
    })
}

/// 建 flip model swapchain，并用 DirectComposition visual 挂到壁纸 HWND。
/// premultiplied alpha + FLIP_SEQUENTIAL 是 24H2 桌面合成树正确采样该窗口的前提。
fn create_composition_swap_chain(
    device: &ID3D11Device,
    hwnd: HWND,
    width: u32,
    height: u32,
) -> Result<CompositionSwapChain> {
    let dxgi_device: IDXGIDevice = device.cast()?;
    let adapter = unsafe { dxgi_device.GetAdapter()? };
    let factory: IDXGIFactory2 = unsafe { adapter.GetParent()? };
    let device_unknown: IUnknown = device.cast()?;

    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: width,
        Height: height,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        Stereo: FALSE,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        Scaling: DXGI_SCALING_STRETCH,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
        Flags: 0,
    };

    let swap_chain: IDXGISwapChain1 = unsafe {
        factory.CreateSwapChainForComposition(&device_unknown, &desc, None::<&IDXGIOutput>)?
    };

    let composition_device: IDCompositionDevice =
        unsafe { DCompositionCreateDevice(&dxgi_device)? };
    let composition_target = unsafe { composition_device.CreateTargetForHwnd(hwnd, true)? };
    let composition_visual = unsafe { composition_device.CreateVisual()? };
    let content: IUnknown = swap_chain.cast()?;
    unsafe {
        composition_visual.SetContent(&content)?;
        composition_target.SetRoot(&composition_visual)?;
        composition_device.Commit()?;
    }

    Ok(CompositionSwapChain {
        swap_chain,
        device: composition_device,
        target: composition_target,
        visual: composition_visual,
    })
}

/// 编译三套 shader 并创建 quad / 采样器 / 光栅化 / blend / fade cbuffer。
/// 全部由 SharedDevice 创建一次，之后克隆给各 surface 共享。
fn create_pipeline(device: &ID3D11Device) -> Result<QuadPipeline> {
    let vs_bytecode = compile_shader(
        SHADER_SOURCE,
        c"vs_main".as_ptr() as *const u8,
        c"vs_5_0".as_ptr() as *const u8,
    )?;
    let ps_bytecode = compile_shader(
        SHADER_SOURCE,
        c"ps_main".as_ptr() as *const u8,
        c"ps_5_0".as_ptr() as *const u8,
    )?;
    let ps_nv12_bytecode = compile_shader(
        SHADER_SOURCE,
        c"ps_nv12".as_ptr() as *const u8,
        c"ps_5_0".as_ptr() as *const u8,
    )?;

    let elements = [
        D3D11_INPUT_ELEMENT_DESC {
            SemanticName: PCSTR(c"POSITION".as_ptr() as *const u8),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: 0,
            InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        },
        D3D11_INPUT_ELEMENT_DESC {
            SemanticName: PCSTR(c"TEXCOORD".as_ptr() as *const u8),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: 8,
            InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        },
    ];

    let mut input_layout: Option<ID3D11InputLayout> = None;
    unsafe {
        device.CreateInputLayout(&elements, &vs_bytecode, Some(&mut input_layout))?;
    }
    let input_layout = input_layout.context("D3D11 input layout 创建失败")?;

    let mut vertex_shader: Option<ID3D11VertexShader> = None;
    unsafe {
        device.CreateVertexShader(&vs_bytecode, None, Some(&mut vertex_shader))?;
    }
    let vertex_shader = vertex_shader.context("D3D11 vertex shader 创建失败")?;

    let mut pixel_shader: Option<ID3D11PixelShader> = None;
    unsafe {
        device.CreatePixelShader(&ps_bytecode, None, Some(&mut pixel_shader))?;
    }
    let pixel_shader = pixel_shader.context("D3D11 pixel shader 创建失败")?;

    let mut nv12_pixel_shader: Option<ID3D11PixelShader> = None;
    unsafe {
        device.CreatePixelShader(&ps_nv12_bytecode, None, Some(&mut nv12_pixel_shader))?;
    }
    let nv12_pixel_shader = nv12_pixel_shader.context("D3D11 NV12 pixel shader 创建失败")?;

    // QUAD 的 TRIANGLESTRIP 绕序与 D3D 默认正面相反，显式关掉背面剔除；
    // 不依赖 RSSetState(NULL) 的隐式默认值，以后换拓扑或改顶点顺序都不会再踩。
    let rasterizer_desc = D3D11_RASTERIZER_DESC {
        FillMode: D3D11_FILL_SOLID,
        CullMode: D3D11_CULL_NONE,
        FrontCounterClockwise: FALSE,
        DepthBias: 0,
        DepthBiasClamp: 0.0,
        SlopeScaledDepthBias: 0.0,
        DepthClipEnable: BOOL(1),
        ScissorEnable: FALSE,
        MultisampleEnable: FALSE,
        AntialiasedLineEnable: FALSE,
    };
    let mut rasterizer_state: Option<ID3D11RasterizerState> = None;
    unsafe {
        device.CreateRasterizerState(&rasterizer_desc, Some(&mut rasterizer_state))?;
    }
    let rasterizer_state = rasterizer_state.context("D3D11 rasterizer state 创建失败")?;

    let buffer_desc = D3D11_BUFFER_DESC {
        ByteWidth: std::mem::size_of_val(&QUAD) as u32,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let buffer_data = D3D11_SUBRESOURCE_DATA {
        pSysMem: QUAD.as_ptr() as *const c_void,
        SysMemPitch: 0,
        SysMemSlicePitch: 0,
    };
    let mut vertex_buffer: Option<ID3D11Buffer> = None;
    unsafe {
        device.CreateBuffer(&buffer_desc, Some(&buffer_data), Some(&mut vertex_buffer))?;
    }
    let vertex_buffer = vertex_buffer.context("D3D11 vertex buffer 创建失败")?;

    let sampler_desc = D3D11_SAMPLER_DESC {
        Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
        AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
        MipLODBias: 0.0,
        MaxAnisotropy: 1,
        ComparisonFunc: D3D11_COMPARISON_ALWAYS,
        BorderColor: [0.0; 4],
        MinLOD: 0.0,
        MaxLOD: 0.0,
    };
    let mut sampler: Option<ID3D11SamplerState> = None;
    unsafe {
        device.CreateSamplerState(&sampler_desc, Some(&mut sampler))?;
    }
    let sampler = sampler.context("D3D11 sampler 创建失败")?;

    // shader 输出预乘色，blend 用 ONE / INV_SRC_ALPHA：pass1(alpha=1) 把 from
    // 完整写入 RT，pass2(alpha=t) 得到 to*t + from*(1-t)，alpha 通道仍收敛到 1。
    let blend_desc = D3D11_BLEND_DESC {
        AlphaToCoverageEnable: FALSE,
        IndependentBlendEnable: FALSE,
        RenderTarget: [D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: BOOL(1),
            SrcBlend: D3D11_BLEND_ONE,
            DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_ONE,
            DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOpAlpha: D3D11_BLEND_OP_ADD,
            RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
        }; 8],
    };
    let mut blend_state: Option<ID3D11BlendState> = None;
    unsafe {
        device.CreateBlendState(&blend_desc, Some(&mut blend_state))?;
    }
    let blend_state = blend_state.context("D3D11 blend state 创建失败")?;

    let fade_desc = D3D11_BUFFER_DESC {
        ByteWidth: std::mem::size_of::<FadeConstants>() as u32,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let mut fade_buffer: Option<ID3D11Buffer> = None;
    unsafe {
        device.CreateBuffer(&fade_desc, None, Some(&mut fade_buffer))?;
    }
    let fade_buffer = fade_buffer.context("D3D11 fade cbuffer 创建失败")?;

    Ok(QuadPipeline {
        vertex_shader,
        pixel_shader,
        nv12_pixel_shader,
        rasterizer_state,
        input_layout,
        vertex_buffer,
        sampler,
        blend_state,
        fade_buffer,
    })
}

/// 运行时编译 HLSL；失败时把 D3DCompile 的错误 blob 一并带进 anyhow，便于定位编译错误。
fn compile_shader(source: &str, entry: *const u8, target: *const u8) -> Result<Vec<u8>> {
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;

    unsafe {
        D3DCompile(
            source.as_ptr() as *const c_void,
            source.len(),
            PCSTR(c"wallpaper.hlsl".as_ptr() as *const u8),
            None,
            None::<&ID3DInclude>,
            PCSTR(entry),
            PCSTR(target),
            0,
            0,
            &mut code,
            Some(&mut errors),
        )
    }
    .map_err(|err| {
        let detail = errors
            .as_ref()
            .map(|blob| unsafe {
                let ptr = blob.GetBufferPointer() as *const u8;
                String::from_utf8_lossy(std::slice::from_raw_parts(ptr, blob.GetBufferSize()))
                    .to_string()
            })
            .unwrap_or_default();
        anyhow!("shader 编译失败: {:?}; {}", err, detail)
    })?;

    let code = code.context("shader 编译结果为空")?;
    unsafe {
        Ok(
            std::slice::from_raw_parts(code.GetBufferPointer() as *const u8, code.GetBufferSize())
                .to_vec(),
        )
    }
}
