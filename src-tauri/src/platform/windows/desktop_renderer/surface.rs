use std::ffi::c_void;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use windows::core::{IUnknown, Interface, BOOL, PCSTR};
use windows::Win32::Foundation::{FALSE, HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{
    ID3DBlob, ID3DInclude, D3D_DRIVER_TYPE_HARDWARE, D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Buffer, ID3D11DepthStencilView, ID3D11Device, ID3D11DeviceContext,
    ID3D11InputLayout, ID3D11PixelShader, ID3D11RasterizerState, ID3D11RenderTargetView,
    ID3D11SamplerState, ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11VertexShader,
    D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_VERTEX_BUFFER, D3D11_BUFFER_DESC,
    D3D11_COMPARISON_ALWAYS, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CULL_NONE, D3D11_FILL_SOLID,
    D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_INPUT_ELEMENT_DESC, D3D11_INPUT_PER_VERTEX_DATA,
    D3D11_RASTERIZER_DESC, D3D11_SAMPLER_DESC, D3D11_SDK_VERSION, D3D11_SUBRESOURCE_DATA,
    D3D11_TEXTURE2D_DESC, D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_USAGE_DEFAULT, D3D11_VIEWPORT,
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

use super::window::NativeWindow;
use crate::platform::wallpaper_renderer::decoder::{decode_frames, DecodedFrame};
use crate::platform::wallpaper_renderer::fit::{fit_rect, FitRect};
use crate::platform::wallpaper_renderer::{FitMode, MonitorRect, WallpaperKind};
use crate::platform::windows::desktop_embedder::EmbedPolicy;

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

float4 ps_main(VS_OUT input) : SV_TARGET {
    float4 color = wallpaper.Sample(linear_sampler, input.uv);
    color.a = 1.0;
    return color;
}
"#;

#[repr(C)]
struct Vertex {
    position: [f32; 2],
    texcoord: [f32; 2],
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

struct QuadPipeline {
    vertex_shader: ID3D11VertexShader,
    pixel_shader: ID3D11PixelShader,
    rasterizer_state: ID3D11RasterizerState,
    input_layout: ID3D11InputLayout,
    vertex_buffer: ID3D11Buffer,
    sampler: ID3D11SamplerState,
}

struct CompositionSwapChain {
    swap_chain: IDXGISwapChain1,
    device: IDCompositionDevice,
    target: IDCompositionTarget,
    visual: IDCompositionVisual,
}

struct GpuFrame {
    view: ID3D11ShaderResourceView,
    width: u32,
    height: u32,
    duration: Duration,
}

struct ActiveWallpaper {
    frames: Vec<GpuFrame>,
    started_at: Instant,
}

impl ActiveWallpaper {
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

pub struct RenderSurface {
    window: NativeWindow,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    swap_chain: IDXGISwapChain1,
    // DirectComposition COM 对象必须活到 RenderSurface 析构；字段只用于持有引用，不直接读取。
    #[allow(dead_code)]
    composition_device: IDCompositionDevice,
    #[allow(dead_code)]
    composition_target: IDCompositionTarget,
    #[allow(dead_code)]
    composition_visual: IDCompositionVisual,
    render_target: Option<ID3D11RenderTargetView>,
    pipeline: QuadPipeline,
    rect: MonitorRect,
    fit_mode: FitMode,
    embedded: bool,
    wallpaper: Option<ActiveWallpaper>,
    // Present / backbuffer 错误只记录一次，避免每帧刷屏
    present_error_logged: bool,
    backbuffer_error_logged: bool,
}

impl RenderSurface {
    pub fn new(rect: MonitorRect) -> Result<Self> {
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
        let composition =
            create_composition_swap_chain(&device, hwnd, rect.width as u32, rect.height as u32)?;

        let mut surface = Self {
            window,
            device,
            context,
            swap_chain: composition.swap_chain,
            composition_device: composition.device,
            composition_target: composition.target,
            composition_visual: composition.visual,
            render_target: None,
            pipeline,
            rect,
            fit_mode: FitMode::Cover,
            embedded: true,
            wallpaper: None,
            present_error_logged: false,
            backbuffer_error_logged: false,
        };
        surface.bind_current_backbuffer()?;
        Ok(surface)
    }

    pub fn set_wallpaper(
        &mut self,
        path: &Path,
        _kind: WallpaperKind,
        fit_mode: FitMode,
    ) -> Result<()> {
        let decoded = decode_frames(path)?;
        let mut frames = Vec::with_capacity(decoded.len());
        for frame in decoded {
            frames.push(self.upload_frame(&frame)?);
        }

        self.wallpaper = Some(ActiveWallpaper {
            frames,
            started_at: Instant::now(),
        });
        self.fit_mode = fit_mode;
        Ok(())
    }

    pub fn set_fit_mode(&mut self, fit_mode: FitMode) {
        self.fit_mode = fit_mode;
    }

    pub fn clear_wallpaper(&mut self) {
        self.wallpaper = None;
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

    pub fn render(&mut self, monitor_id: &str) {
        if let Err(err) = self.bind_current_backbuffer() {
            if !self.backbuffer_error_logged {
                log::error!(
                    "[Renderer] {} 当前 backbuffer 绑定失败: {:#}",
                    monitor_id,
                    err
                );
                self.backbuffer_error_logged = true;
            }
            return;
        }
        self.backbuffer_error_logged = false;

        let Some(render_target) = &self.render_target else {
            return;
        };

        unsafe {
            self.context
                .ClearRenderTargetView(render_target, &[0.08, 0.12, 0.18, 1.0]);
        }

        if let Some(wallpaper) = &self.wallpaper {
            let frame = wallpaper.current_frame(Instant::now());
            let pipeline = &self.pipeline;
            let vertex_buffers = [Some(pipeline.vertex_buffer.clone())];
            let strides = [std::mem::size_of::<Vertex>() as u32];
            let offsets = [0u32];
            let samplers = [Some(pipeline.sampler.clone())];
            let views = [Some(frame.view.clone())];
            let FitRect {
                x,
                y,
                width,
                height,
            } = fit_rect(
                self.fit_mode,
                frame.width,
                frame.height,
                self.rect.width as u32,
                self.rect.height as u32,
            );
            let viewport = D3D11_VIEWPORT {
                TopLeftX: x,
                TopLeftY: y,
                Width: width,
                Height: height,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            };

            unsafe {
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
                self.context.PSSetShader(&pipeline.pixel_shader, None);
                self.context.PSSetSamplers(0, Some(&samplers));
                self.context.PSSetShaderResources(0, Some(&views));
                // QUAD 顶点顺序与 D3D 默认正面绕序相反，不显式关背面剔除会丢掉全部三角形
                self.context.RSSetState(&pipeline.rasterizer_state);
                self.context.RSSetViewports(Some(&[viewport]));
                self.context.Draw(4, 0);
            }
        }

        let present = unsafe { self.swap_chain.Present(1, DXGI_PRESENT(0)) };
        if let Err(err) = present.ok() {
            if !self.present_error_logged {
                log::warn!("[Renderer] {} Present 失败: {:?}", monitor_id, err);
                self.present_error_logged = true;
            }
        }
    }

    pub fn destroy(&mut self) {
        self.wallpaper = None;
        if self.embedded {
            crate::platform::windows::desktop_embedder::unembed_from_desktop(
                self.window.hwnd().0 as isize,
            );
            self.embedded = false;
        }
    }

    fn upload_frame(&self, frame: &DecodedFrame) -> Result<GpuFrame> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: frame.width,
            Height: frame.height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let data = D3D11_SUBRESOURCE_DATA {
            pSysMem: frame.bgra.as_ptr() as *const c_void,
            SysMemPitch: frame.width * 4,
            SysMemSlicePitch: 0,
        };

        let mut texture: Option<ID3D11Texture2D> = None;
        unsafe {
            self.device
                .CreateTexture2D(&desc, Some(&data), Some(&mut texture))?;
        }
        let texture = texture.context("D3D11 frame texture 创建失败")?;

        // SRV 内部持有 texture 引用，无需在 GpuFrame 里再保存一份
        let mut view: Option<ID3D11ShaderResourceView> = None;
        unsafe {
            self.device
                .CreateShaderResourceView(&texture, None, Some(&mut view))?;
        }
        let view = view.context("D3D11 frame shader resource view 创建失败")?;

        Ok(GpuFrame {
            view,
            width: frame.width,
            height: frame.height,
            duration: frame.duration,
        })
    }
}

impl Drop for RenderSurface {
    fn drop(&mut self) {
        self.destroy();
    }
}

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

    Ok(QuadPipeline {
        vertex_shader,
        pixel_shader,
        rasterizer_state,
        input_layout,
        vertex_buffer,
        sampler,
    })
}

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
