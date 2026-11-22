use super::contracts::FitMode;

/// 源图映射后的目标矩形。`x` / `y` 是相对显示器左上角的偏移，**可能为负**
/// （cover 放大后靠裁剪对齐、center 居中）：负值意味着矩形溢出到显示器之外，
/// 由后续裁剪处理，而不是在这里截断。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FitRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// 把源图按 fit 模式映射到目标显示器矩形，返回值可能超出目标区域（cover / center 靠裁剪处理）。
pub fn fit_rect(
    fit: FitMode,
    source_width: u32,
    source_height: u32,
    target_width: u32,
    target_height: u32,
) -> FitRect {
    let target = FitRect {
        x: 0.0,
        y: 0.0,
        width: target_width as f32,
        height: target_height as f32,
    };

    // 退化尺寸直接铺满目标：既避免后续除零，也保证界面不会出现空矩形。
    if source_width == 0 || source_height == 0 || target_width == 0 || target_height == 0 {
        return target;
    }

    let source_width = source_width as f32;
    let source_height = source_height as f32;
    let target_width = target_width as f32;
    let target_height = target_height as f32;

    match fit {
        // CSS object-fit 没有 stretch，前端写 objectFit: "stretch" 时也会回退到 fill
        FitMode::Fill | FitMode::Stretch => target,
        FitMode::Center => FitRect {
            x: (target_width - source_width) / 2.0,
            y: (target_height - source_height) / 2.0,
            width: source_width,
            height: source_height,
        },
        FitMode::Cover => scaled_rect(
            source_width,
            source_height,
            target_width,
            target_height,
            true,
        ),
        FitMode::Contain => scaled_rect(
            source_width,
            source_height,
            target_width,
            target_height,
            false,
        ),
    }
}

/// 按等比缩放计算居中后的矩形。`cover = true` 取较大的缩放比，保证铺满目标
///（超出部分由裁剪丢弃）；`false` 取较小的缩放比，保证完整显示（留黑边）。
/// 两者都把结果相对目标居中，因此 `x` / `y` 可能为负。
fn scaled_rect(
    source_width: f32,
    source_height: f32,
    target_width: f32,
    target_height: f32,
    cover: bool,
) -> FitRect {
    let width_scale = target_width / source_width;
    let height_scale = target_height / source_height;
    let scale = if cover {
        width_scale.max(height_scale)
    } else {
        width_scale.min(height_scale)
    };

    let width = source_width * scale;
    let height = source_height * scale;
    FitRect {
        x: (target_width - width) / 2.0,
        y: (target_height - height) / 2.0,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_and_stretch_ignore_aspect_ratio() {
        let expected = FitRect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        assert_eq!(fit_rect(FitMode::Fill, 800, 600, 1920, 1080), expected);
        assert_eq!(fit_rect(FitMode::Stretch, 800, 600, 1920, 1080), expected);
    }

    #[test]
    fn cover_overflows_and_centers() {
        let rect = fit_rect(FitMode::Cover, 400, 300, 1920, 1080);
        assert!((rect.width - 1920.0).abs() < 0.01);
        assert!((rect.height - 1440.0).abs() < 0.01);
        assert!((rect.x - 0.0).abs() < 0.01);
        assert!((rect.y - (-180.0)).abs() < 0.01);
    }

    #[test]
    fn contain_letterboxes() {
        let rect = fit_rect(FitMode::Contain, 400, 300, 1920, 1080);
        assert!((rect.width - 1440.0).abs() < 0.01);
        assert!((rect.height - 1080.0).abs() < 0.01);
        assert!((rect.x - 240.0).abs() < 0.01);
        assert!((rect.y - 0.0).abs() < 0.01);
    }

    #[test]
    fn center_keeps_original_size() {
        let rect = fit_rect(FitMode::Center, 800, 600, 1920, 1080);
        assert_eq!(
            rect,
            FitRect {
                x: 560.0,
                y: 240.0,
                width: 800.0,
                height: 600.0,
            }
        );
    }

    #[test]
    fn zero_dimension_falls_back_to_target() {
        assert_eq!(
            fit_rect(FitMode::Cover, 0, 0, 1920, 1080),
            FitRect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            }
        );
    }
}
