//! 全屏 quad 的 HLSL 与 D3D11 绘制管线资源。
//!
//! shader 源码、与 GPU 逐字节对齐的数据结构、以及 9 个管线对象的创建集中在此。
//! 由 `SharedDevice::new()` 创建一次并把 `QuadPipeline` 克隆给所有 surface。

/// 编译三套 shader 并创建 quad / 采样器 / 光栅化 / blend / fade cbuffer。
/// 全部由 SharedDevice 创建一次，之后克隆给各 surface 共享。
use std::ffi::c_void;

use anyhow::{anyhow, Context, Result};
use windows::core::{BOOL, PCSTR};
use windows::Win32::Foundation::FALSE;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{ID3DBlob, ID3DInclude};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11BlendState, ID3D11Buffer, ID3D11Device, ID3D11InputLayout, ID3D11PixelShader,
    ID3D11RasterizerState, ID3D11SamplerState, ID3D11VertexShader, D3D11_BIND_CONSTANT_BUFFER,
    D3D11_BIND_VERTEX_BUFFER, D3D11_BLEND_DESC, D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ONE,
    D3D11_BLEND_OP_ADD, D3D11_BUFFER_DESC, D3D11_COLOR_WRITE_ENABLE_ALL, D3D11_COMPARISON_ALWAYS,
    D3D11_CULL_NONE, D3D11_FILL_SOLID, D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_INPUT_ELEMENT_DESC,
    D3D11_INPUT_PER_VERTEX_DATA, D3D11_RASTERIZER_DESC, D3D11_RENDER_TARGET_BLEND_DESC,
    D3D11_SAMPLER_DESC, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R32G32_FLOAT;

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

// 一次绑定服务两个像素着色器：fade_alpha 供 ps_main / ps_nv12 混合，
// 其余 5 个 float 供 ps_nv12 做 YUV→RGB（系数由视频元数据推导）。
// 三个 float4 槽位共 48 字节，与 Rust 侧 FadeConstants 对齐。
cbuffer Fade : register(b0) {
    float4 fade_params;   // x = fade_alpha，yzw 保留
    float4 yuv_params;    // x = y_offset, y = y_scale, z = r_v, w = g_u
    float4 yuv_params2;   // x = g_v, y = b_u, zw 保留
};

float4 ps_main(VS_OUT input) : SV_TARGET {
    float4 color = wallpaper.Sample(linear_sampler, input.uv);
    // swapchain 声明为 premultiplied alpha，输出必须预乘；否则两 pass 混合会
    // 把 RT 的 alpha 拉到 <1，桌面合成阶段整屏变暗。
    color.rgb *= fade_params.x;
    color.a = fade_params.x;
    return color;
}

Texture2D y_plane : register(t0);
Texture2D uv_plane : register(t1);

float4 ps_nv12(VS_OUT input) : SV_TARGET {
    // NV12 → RGB，系数由视频流的颜色矩阵 / range 经 cbuffer 传入，
    // 全部在 GPU 侧完成，避免 CPU 逐像素转换。
    float y = saturate((y_plane.Sample(linear_sampler, input.uv).r - yuv_params.x) * yuv_params.y);
    float2 uv = uv_plane.Sample(linear_sampler, input.uv).rg - 0.5;
    float r = saturate(y + yuv_params.z * uv.y);
    float g = saturate(y - yuv_params.w * uv.x - yuv_params2.x * uv.y);
    float b = saturate(y + yuv_params2.y * uv.x);
    return float4(float3(r, g, b) * fade_params.x, fade_params.x);
}
"#;

/// 与 input layout（POSITION / TEXCOORD）一一对应的全屏 quad 顶点。
///
/// `#[repr(C)]` 不可省：Rust 默认布局不保证字段顺序与偏移，而 GPU 直接按字节读取
/// 这块顶点缓冲。此处必须与 HLSL 的 `VS_IN` 以及 `D3D11_INPUT_ELEMENT_DESC` 的
/// `R32G32_FLOAT` / 偏移 0 与 8 三处严格一致。
#[repr(C)]
pub(super) struct Vertex {
    position: [f32; 2],
    texcoord: [f32; 2],
}

/// 与 HLSL `cbuffer Fade` 逐字节对齐（3 个 float4 = 48 字节）。
///
/// 原来只有 fade_alpha + 3 个 pad；现在复用空闲槽位承载 YUV→RGB 系数，
/// 避免为颜色标准再开一个 cbuffer 或编译 4 个 shader 变体。
#[repr(C)]
pub(super) struct FadeConstants {
    pub(super) alpha: f32,
    pub(super) _pad0: [f32; 3],
    /// y_offset, y_scale, r_v, g_u
    pub(super) yuv0: [f32; 4],
    /// g_v, b_u, 0, 0
    pub(super) yuv1: [f32; 4],
}

/// 铺满 NDC 视口（x/y ∈ [-1,1]）的四个顶点；纹理坐标上下翻转，
/// 因为 D3D 纹理原点在左上而 NDC 的 y 向上。
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
/// 每个字段对应绘制流程的一步：顶点变换 → 光栅化 → 像素采样 → 混合输出。
#[derive(Clone)]
pub struct QuadPipeline {
    /// 顶点着色器 `vs_main`：把 quad 的 NDC 坐标与 UV 原样透传，无矩阵变换。
    pub(super) vertex_shader: ID3D11VertexShader,
    /// 图片像素着色器 `ps_main`：单张 BGRA 纹理采样 × fade_alpha。
    pub(super) pixel_shader: ID3D11PixelShader,
    /// 视频像素着色器 `ps_nv12`：Y/UV 双平面采样 + cbuffer 系数转 RGB × fade_alpha。
    pub(super) nv12_pixel_shader: ID3D11PixelShader,
    /// 光栅化状态：关背面剔除（TRIANGLESTRIP 绕序与默认正面相反）、实心填充。
    pub(super) rasterizer_state: ID3D11RasterizerState,
    /// 输入布局：声明 POSITION/TEXCOORD 两个 float2 在顶点缓冲中的偏移（0/8 字节）。
    pub(super) input_layout: ID3D11InputLayout,
    /// 顶点缓冲：QUAD 的四个顶点，一次 Draw 铺满整个视口。
    pub(super) vertex_buffer: ID3D11Buffer,
    /// 采样器状态：双线性过滤 + 越界钳制，供两个像素着色器共用（绑定在 s0）。
    pub(super) sampler: ID3D11SamplerState,
    /// 混合状态：ONE / INV_SRC_ALPHA，实现预乘 alpha 的两 pass crossfade。
    pub(super) blend_state: ID3D11BlendState,
    /// 常量缓冲：fade_alpha + YUV→RGB 系数（3 个 float4），绑定在 b0 驱动两个像素着色器。
    pub(super) fade_buffer: ID3D11Buffer,
}

pub(super) fn create_pipeline(device: &ID3D11Device) -> Result<QuadPipeline> {
    // 1. 编译三套 shader：共用顶点着色器，图片 / 视频各一个像素着色器。
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

    // 2. 顶点布局：两个 float2 属性与 HLSL 的 VS_IN 一一对应，偏移 0 / 8。
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

    // 3. 用顶点着色器字节码验证布局并创建 input layout（D3D 要求二者匹配）。
    let mut input_layout: Option<ID3D11InputLayout> = None;
    unsafe {
        device.CreateInputLayout(&elements, &vs_bytecode, Some(&mut input_layout))?;
    }
    let input_layout = input_layout.context("D3D11 input layout 创建失败")?;

    // 4. 把三套字节码分别包装成 D3D shader 对象。
    let mut vertex_shader: Option<ID3D11VertexShader> = None;
    unsafe {
        device.CreateVertexShader(&vs_bytecode, None, Some(&mut vertex_shader))?;
    }
    let vertex_shader = vertex_shader.context("D3D11 vertex shader 创建失败")?;

    // 图片像素着色器（BGRA 单纹理）。
    let mut pixel_shader: Option<ID3D11PixelShader> = None;
    unsafe {
        device.CreatePixelShader(&ps_bytecode, None, Some(&mut pixel_shader))?;
    }
    let pixel_shader = pixel_shader.context("D3D11 pixel shader 创建失败")?;

    // 视频像素着色器（NV12 双平面）。
    let mut nv12_pixel_shader: Option<ID3D11PixelShader> = None;
    unsafe {
        device.CreatePixelShader(&ps_nv12_bytecode, None, Some(&mut nv12_pixel_shader))?;
    }
    let nv12_pixel_shader = nv12_pixel_shader.context("D3D11 NV12 pixel shader 创建失败")?;

    // 5. 光栅化状态：实心填充、无深度、单采样。
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

    // 6. 顶点缓冲：把 QUAD 的四个顶点一次性上传为默认用途的 vertex buffer。
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

    // 7. 采样器：线性过滤 + 三轴钳制，UV 越界时取边缘而非回绕。
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

    // 8. 混合状态：颜色与 alpha 走同一套混合方程，8 个 RT 共用配置（实际只用 RT0）。
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

    // 9. fade 常量缓冲：启动时只分配，每帧由 renderer 经 Map 写入 fade_alpha。
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
    // 两个 blob：成功时 code 装字节码，失败时 errors 装编译器诊断文本。
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;

    unsafe {
        D3DCompile(
            // 源码按 (指针, 长度) 传，可含任意字节；文件名仅用于错误信息定位。
            source.as_ptr() as *const c_void,
            source.len(),
            PCSTR(c"wallpaper.hlsl".as_ptr() as *const u8),
            None,
            None::<&ID3DInclude>,
            // entry 是 HLSL 入口函数名，target 是着色器模型（vs_5_0 / ps_5_0）。
            PCSTR(entry),
            PCSTR(target),
            // flags1 / flags2：无特殊编译选项（调试或优化开关都走默认）。
            0,
            0,
            // 输出：成功字节码与错误诊断分别落到两个 blob。
            &mut code,
            Some(&mut errors),
        )
    }
    .map_err(|err| {
        // 从错误 blob 读出编译器诊断文本，拼进 anyhow 便于定位 HLSL 行号。
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

    // 把成功 blob 的字节拷进 Vec：blob 随函数结束释放，字节码要活到 CreateXxxShader。
    let code = code.context("shader 编译结果为空")?;
    unsafe {
        Ok(
            std::slice::from_raw_parts(code.GetBufferPointer() as *const u8, code.GetBufferSize())
                .to_vec(),
        )
    }
}
