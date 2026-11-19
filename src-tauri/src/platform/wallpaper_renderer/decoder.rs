use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Result};
use image::codecs::gif::GifDecoder;
use image::codecs::webp::WebPDecoder;
use image::{AnimationDecoder, DynamicImage, Frame};

#[derive(Clone, Debug)]
pub struct DecodedFrame {
    pub bgra: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub duration: Duration,
}

pub fn decode_frames(path: &Path) -> Result<Vec<DecodedFrame>> {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "gif" => {
            let decoder = GifDecoder::new(BufReader::new(File::open(path)?))?;
            decode_animated(decoder)
        }
        "webp" => {
            let decoder = WebPDecoder::new(BufReader::new(File::open(path)?))?;
            decode_animated(decoder)
        }
        _ => Ok(vec![build_frame(image::open(path)?, Duration::ZERO)]),
    }
}

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

fn frame_duration(frame: &Frame) -> Duration {
    let duration: Duration = frame.delay().into();
    // GIF 的 0 延迟未定义，主流播放器按 100ms 处理，否则帧选择逻辑会卡在首帧
    if duration.is_zero() {
        Duration::from_millis(100)
    } else {
        duration
    }
}

fn build_frame(image: DynamicImage, duration: Duration) -> DecodedFrame {
    let width = image.width();
    let height = image.height();
    let rgba = image.to_rgba8().into_raw();
    let bgra = rgba_to_bgra(&rgba);

    DecodedFrame {
        bgra,
        width,
        height,
        duration,
    }
}

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
