use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use image::codecs::gif::GifDecoder;
use image::codecs::webp::WebPDecoder;
use image::{AnimationDecoder, DynamicImage, Frame};

/// 一帧解码结果。像素固定为 BGRA8888，与 D3D11 的 `DXGI_FORMAT_B8G8R8A8_UNORM`
/// 内存布局一致，上传纹理时无需再逐像素换通道。`bgra` 长度应为 `width * height * 4`。
#[derive(Clone, Debug)]
pub struct DecodedFrame {
    /// BGRA8888 像素缓冲（由 RGBA 交换 R/B 得来）。
    pub bgra: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// 本帧显示时长：GIF / 视频取自帧延迟；静态图为 `Duration::ZERO`，表示常驻不轮换。
    pub duration: Duration,
}

/// 按扩展名把文件解码为帧序列：GIF / WebP 逐帧解码，其余按静态图处理为单帧。
/// 这是与 GPU 无关的纯 CPU 逻辑，供渲染线程使用，也能在 macOS 下单测。
pub fn decode_frames(path: &Path) -> Result<Vec<DecodedFrame>> {
    let started = Instant::now();
    // 只按扩展名分派，不探测文件内容：扩展名与容器不一致时交由解码器报错。
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let label = file_label(path);

    let frames = match ext.as_str() {
        "gif" => {
            let decoder = GifDecoder::new(BufReader::new(File::open(path)?))?;
            decode_animated(decoder)?
        }
        "webp" => {
            let decoder = WebPDecoder::new(BufReader::new(File::open(path)?))?;
            decode_animated(decoder)?
        }
        _ => {
            let open_started = Instant::now();
            let image = image::open(path)?;
            log::debug!(
                "[Decode] 静态图 image::open file={} ms={:.1}",
                label,
                elapsed_ms(open_started)
            );
            vec![build_frame(image, Duration::ZERO)]
        }
    };

    log::debug!(
        "[Decode] 解码完成 file={} frames={} total_ms={:.1}",
        label,
        frames.len(),
        elapsed_ms(started)
    );
    Ok(frames)
}

/// 逐帧解码动画图并保留每帧自身的延迟。
/// 空帧序列视为错误，避免下游帧选择逻辑无帧可取。
fn decode_animated<'a, D>(decoder: D) -> Result<Vec<DecodedFrame>>
where
    D: AnimationDecoder<'a>,
{
    let mut frames = Vec::new();

    for frame in decoder.into_frames() {
        let frame = frame?;
        let duration = frame_duration(&frame);
        frames.push(build_frame(
            DynamicImage::ImageRgba8(frame.into_buffer()),
            duration,
        ));
    }

    if frames.is_empty() {
        bail!("animated image has no frames");
    }

    Ok(frames)
}

/// 读取帧延迟，并对 GIF 的「零延迟」做归一化（见函数内注释）。
fn frame_duration(frame: &Frame) -> Duration {
    let duration: Duration = frame.delay().into();
    // GIF 的 0 延迟未定义，主流播放器按 100ms 处理，否则帧选择逻辑会卡在首帧
    if duration.is_zero() {
        Duration::from_millis(100)
    } else {
        duration
    }
}

/// 把 `DynamicImage` 转成 BGRA 帧；两段耗时分别打点，便于定位解码瓶颈在
/// 「统一像素格式」还是「通道交换」。
fn build_frame(image: DynamicImage, duration: Duration) -> DecodedFrame {
    let width = image.width();
    let height = image.height();

    let rgba_started = Instant::now();
    // 先统一到 RGBA8，下游只需处理一种固定的内存布局。
    let rgba = image.to_rgba8().into_raw();
    let rgba_ms = elapsed_ms(rgba_started);

    let bgra_started = Instant::now();
    // 再交换 R/B 变成 D3D11 期望的 BGRA；此处会产生一次整帧拷贝。
    let bgra = rgba_to_bgra(&rgba);
    log::debug!(
        "[Decode] 像素转换 {}x{} to_rgba8_ms={:.1} rgba_to_bgra_ms={:.1}",
        width,
        height,
        rgba_ms,
        elapsed_ms(bgra_started)
    );

    DecodedFrame {
        bgra,
        width,
        height,
        duration,
    }
}

/// 交换 R / B 通道：`image` crate 输出 RGBA，而 D3D11 纹理期望 BGRA 内存布局。
/// 只换 0/2 通道，alpha 原样保留以维持透明度语义。
fn rgba_to_bgra(rgba: &[u8]) -> Vec<u8> {
    let mut bgra = vec![0u8; rgba.len()];
    for (src, dst) in rgba.chunks_exact(4).zip(bgra.chunks_exact_mut(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }
    bgra
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("<unknown>")
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use image::codecs::png::PngEncoder;
    use image::{Delay, ExtendedColorType, ImageEncoder, RgbaImage};

    use super::*;

    struct TempPng {
        path: std::path::PathBuf,
    }

    impl TempPng {
        fn new() -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let id = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "mini-wallpaper-decoder-{}-{}-{}.png",
                std::process::id(),
                nanos,
                id
            ));
            let file = fs::File::create(&path).expect("create temp png");
            PngEncoder::new(file)
                .write_image(&[255, 0, 0, 255], 1, 1, ExtendedColorType::Rgba8)
                .expect("write temp png");
            Self { path }
        }
    }

    impl Drop for TempPng {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    #[test]
    fn rgba_to_bgra_swaps_red_and_blue() {
        assert_eq!(rgba_to_bgra(&[1, 2, 3, 4]), vec![3, 2, 1, 4]);
    }

    #[test]
    fn decode_static_image_keeps_original_size() {
        let temp = TempPng::new();
        let frames = decode_frames(&temp.path).expect("decode png");

        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].width, 1);
        assert_eq!(frames[0].height, 1);
        assert_eq!(frames[0].bgra.len(), 4);
        assert_eq!(frames[0].duration, Duration::ZERO);
    }

    #[test]
    fn frame_duration_reads_delay() {
        let frame = Frame::new(RgbaImage::new(1, 1));
        assert_eq!(frame_duration(&frame), Duration::from_millis(100));
    }

    #[test]
    fn frame_duration_normalizes_nonzero_delay() {
        let frame = Frame::from_parts(
            RgbaImage::new(1, 1),
            0,
            0,
            Delay::from_numer_denom_ms(250, 1),
        );
        assert_eq!(frame_duration(&frame), Duration::from_millis(250));
    }
}
