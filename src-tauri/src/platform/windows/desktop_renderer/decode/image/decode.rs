//! 图片 / GIF / 动态 WebP 的一次性解码 worker。
//!
//! 与 video.rs 的常驻 worker 不同：图片解码是一次性 CPU 密集工作（`image::open`
//! 4K 约 75ms），放在渲染线程会冻结整组 surface。这里把它拆成一个纯函数，由
//! renderer 在每张图片换图时 spawn 一次性线程调用，完成后线程即退出、device clone
//! 随之释放；渲染线程只做结果回收与 attach。
//!
//! 不做 COM 初始化：本函数只用 D3D11 device 的 CreateTexture2D / CreateShaderResourceView
//! 与纯 CPU 解码，二者都不依赖 COM apartment（video worker 同样未初始化）。
//! 不持有任何跨线程状态，因此连点产生多个在途 worker 时无需加锁；过期结果由渲染线程
//! 按 generation 丢弃。
//!
//! JPEG 走 turbojpeg 的 DCT 缩放（M/8 档位）：高分辨率图按目标尺寸直接解码，
//! 避免全尺寸 IDCT。其它格式仍走 image crate 的 decode_frames。

use std::ffi::c_void;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use turbojpeg::{Decompressor, Image as TjImage, PixelFormat, ScalingFactor};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, D3D11_BIND_SHADER_RESOURCE, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};

use super::super::{GpuFrame, SharedFrames};
use crate::platform::wallpaper_renderer::decoder::{decode_frames, DecodedFrame};

/// 一次解码 + 一次纹理上传，返回整组共用的帧序列。
/// 在一次性 worker 线程上执行；`device` 是渲染线程 device 的 clone，生命周期由本函数借用。
/// `target` 为 (宽, 高)：JPEG 用它选 DCT 缩放档位，其余格式忽略。
pub(crate) fn decode_and_upload(
    device: &ID3D11Device,
    path: &Path,
    target: Option<(u32, u32)>,
) -> Result<SharedFrames> {
    let started = Instant::now();
    let decode_started = Instant::now();
    let is_jpeg = matches!(
        path.extension()
            .and_then(|v| v.to_str())
            .map(|v| v.to_ascii_lowercase())
            .as_deref(),
        Some("jpg") | Some("jpeg")
    );
    // 只有 JPEG 且给了目标尺寸时才走 turbojpeg 缩放；其余格式与原行为完全一致。
    let decoded = if is_jpeg && target.is_some() {
        decode_jpeg_scaled(path, target)?
    } else {
        decode_frames(path)?
    };
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

/// 按目标尺寸选 turbojpeg 的 M/8 缩放档位。
///
/// DCT 缩放是等比的，档位只有 M/8（M=1..8）。取**最小的 M** 使得缩放后宽高仍
/// 不小于目标——M 越小解出的像素越少、IDCT 越省。若 M=8 仍小于目标（源图本身
/// 不够大），退化为原尺寸解码； Fill 拉伸会放大，但这不是缩放能解决的问题。
fn pick_scaling_factor(source_w: u32, source_h: u32, target: (u32, u32)) -> ScalingFactor {
    let (tw, th) = target;
    for numerator in 1..=8u32 {
        // ceil(source * numerator / 8) >= target，用整数运算避免浮点误差
        let scaled_w = source_w.saturating_mul(numerator).saturating_add(7) / 8;
        let scaled_h = source_h.saturating_mul(numerator).saturating_add(7) / 8;
        if scaled_w >= tw && scaled_h >= th {
            return ScalingFactor::new(numerator as usize, 8);
        }
    }
    ScalingFactor::ONE
}

/// JPEG 解码 + DCT 缩放，直接输出 BGRX（D3D11 的 BGRA 布局，X 字节未定义但 JPEG
/// 无 alpha，正好省掉 image crate 路径里的 rgba_to_bgra 整帧拷贝）。
fn decode_jpeg_scaled(path: &Path, target: Option<(u32, u32)>) -> Result<Vec<DecodedFrame>> {
    let jpeg = std::fs::read(path).with_context(|| format!("读取 JPEG 失败 {:?}", path))?;
    let mut decoder = Decompressor::new().context("turbojpeg 解压器创建失败")?;
    let header = decoder
        .read_header(&jpeg)
        .with_context(|| format!("JPEG 头解析失败 {:?}", path))?;
    let scale = target
        .map(|(tw, th)| pick_scaling_factor(header.width as u32, header.height as u32, (tw, th)))
        .unwrap_or(ScalingFactor::ONE);
    let scaled = header.scaled(scale);
    let width = scaled.width as u32;
    let height = scaled.height as u32;

    let mut image = TjImage {
        pixels: vec![0u8; 4 * width as usize * height as usize],
        width: scaled.width,
        pitch: 4 * scaled.width,
        height: scaled.height,
        format: PixelFormat::BGRX,
    };
    if scale != ScalingFactor::ONE {
        decoder.set_scaling_factor(scale)?;
    }
    decoder
        .decompress(&jpeg, image.as_deref_mut())
        .with_context(|| format!("JPEG 解码失败 {:?}", path))?;

    log::info!(
        "[Decode] JPEG DCT 缩放 file={} {}x{} -> {}x{} ({}/8)",
        path.file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("<unknown>"),
        header.width,
        header.height,
        width,
        height,
        scale.num() * 8 / scale.denom()
    );

    Ok(vec![DecodedFrame {
        bgra: image.pixels,
        width,
        height,
        duration: Duration::ZERO,
    }])
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}
