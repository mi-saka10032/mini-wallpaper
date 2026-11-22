//! 图片 / GIF / 动态 WebP 的一次性解码 worker。
//!
//! 与 video.rs 的常驻 worker 不同：图片解码是一次性 CPU 密集工作（`image::open`
//! 4K 约 75ms），放在渲染线程会冻结整组 surface。这里把它拆成一个纯函数，由
//! renderer 在 cache miss 时 spawn 一次性线程调用，完成后线程即退出、device clone
//! 随之释放；FrameCache 仍独占在渲染线程，命中时根本不进这里。
//!
//! 不做 COM 初始化：本函数只用 D3D11 device 的 CreateTexture2D / CreateShaderResourceView
//! 与 image crate 的纯 CPU 解码，二者都不依赖 COM apartment（video worker 同样未初始化）。
//! 不持有任何跨线程状态，因此连点产生多个在途 worker 时无需加锁；过期结果由渲染线程
//! 按 generation 丢弃。

use std::ffi::c_void;
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, D3D11_BIND_SHADER_RESOURCE, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};

use super::surface::{GpuFrame, SharedFrames};
use crate::platform::wallpaper_renderer::decoder::{decode_frames, DecodedFrame};
use crate::platform::wallpaper_renderer::WallpaperKind;

/// 一次解码 + 一次纹理上传，返回整组共用的帧序列。
/// 在一次性 worker 线程上执行；`device` 是渲染线程 device 的 clone，生命周期由本函数借用。
pub(crate) fn decode_and_upload(
    device: &ID3D11Device,
    path: &Path,
    _kind: WallpaperKind,
) -> Result<SharedFrames> {
    let started = Instant::now();
    let decode_started = Instant::now();
    let decoded = decode_frames(path)?;
    let decode_ms = elapsed_ms(decode_started);
    if decoded.is_empty() {
        anyhow::bail!("解码结果为空: {:?}", path);
    }

    let upload_started = Instant::now();
    let mut frames = Vec::with_capacity(decoded.len());
    for frame in &decoded {
        frames.push(upload_frame(device, frame)?);
    }
    let upload_ms = elapsed_ms(upload_started);
    log::info!(
        "[Upload] 图片就绪 file={} frames={} decode_ms={:.1} upload_ms={:.1} total_ms={:.1}",
        path.file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("<unknown>"),
        frames.len(),
        decode_ms,
        upload_ms,
        elapsed_ms(started)
    );

    Ok(SharedFrames::new(frames))
}

/// 单帧解码结果上传显存：建 DEFAULT 纹理并以初始数据填充，再建 SRV。
/// 纹理只由 SRV 持有（见 GpuFrame），这里不重复存一份以简化引用管理。
fn upload_frame(device: &ID3D11Device, frame: &DecodedFrame) -> Result<GpuFrame> {
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

    let texture_started = Instant::now();
    let mut texture: Option<windows::Win32::Graphics::Direct3D11::ID3D11Texture2D> = None;
    unsafe {
        device.CreateTexture2D(&desc, Some(&data), Some(&mut texture))?;
    }
    let texture_ms = elapsed_ms(texture_started);
    let texture = texture.context("D3D11 frame texture 创建失败")?;

    // SRV 内部持有 texture 引用，无需在 GpuFrame 里再保存一份
    let srv_started = Instant::now();
    let mut view: Option<windows::Win32::Graphics::Direct3D11::ID3D11ShaderResourceView> = None;
    unsafe {
        device.CreateShaderResourceView(&texture, None, Some(&mut view))?;
    }
    log::debug!(
        "[Upload] texture {}x{} create_ms={:.1} srv_ms={:.1}",
        frame.width,
        frame.height,
        texture_ms,
        elapsed_ms(srv_started)
    );
    let view = view.context("D3D11 frame shader resource view 创建失败")?;

    Ok(GpuFrame {
        view,
        width: frame.width,
        height: frame.height,
        duration: frame.duration,
    })
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}
