//! 跨屏几何：把「一张壁纸横跨所有显示器」换算成每屏的裁剪视口。
//!
//! ## 坐标约定（Q2）
//!
//! 全部使用**物理像素的虚拟桌面坐标**，与 `CreateWindowExW` / `MonitorRect` 一致。
//! Windows 允许负坐标，主屏不一定在左上角，所以 union 原点可能是负数。
//!
//! ## 与 WebView 的视觉一致性（Q1）
//!
//! WebView 侧把 `<img>` 拉伸到 `totalWidth × totalHeight`（`object-fit: fill`），
//! 再用容器 `left: -offsetX / top: -offsetY` 裁出本屏那一块。
//! 这里让 offset 相对 **union 左上角**（而不是主屏），并把 fit 固定为 `Fill`，
//! 等价于同一套几何：任一屏看到的都是同一张全图被拉伸后的对应区域。

use super::contracts::{ExtendViewport, MonitorRect};

/// 所有显示器矩形的并集；空集合返回 `None`。
pub fn union_rect(rects: &[MonitorRect]) -> Option<MonitorRect> {
    let first = *rects.first()?;
    let mut min_x = first.x;
    let mut min_y = first.y;
    let mut max_x = first.x.saturating_add(first.width);
    let mut max_y = first.y.saturating_add(first.height);

    for rect in &rects[1..] {
        min_x = min_x.min(rect.x);
        min_y = min_y.min(rect.y);
        max_x = max_x.max(rect.x.saturating_add(rect.width));
        max_y = max_y.max(rect.y.saturating_add(rect.height));
    }

    Some(MonitorRect {
        x: min_x,
        y: min_y,
        width: max_x.saturating_sub(min_x),
        height: max_y.saturating_sub(min_y),
    })
}

/// 单个显示器在 extend 组内的裁剪参数。
pub fn extend_viewport(rect: MonitorRect, union: MonitorRect) -> ExtendViewport {
    ExtendViewport {
        offset_x: rect.x - union.x,
        offset_y: rect.y - union.y,
        total_width: union.width,
        total_height: union.height,
        monitor_width: rect.width,
        monitor_height: rect.height,
    }
}

/// 为整组显示器生成裁剪参数；由调用方保证 union 来自同一批 rects。
pub fn group_viewports(
    monitors: &[(String, MonitorRect)],
) -> Option<Vec<(String, ExtendViewport)>> {
    let union = union_rect(&monitors.iter().map(|(_, r)| *r).collect::<Vec<_>>())?;
    Some(
        monitors
            .iter()
            .map(|(id, rect)| (id.clone(), extend_viewport(*rect, union)))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, width: i32, height: i32) -> MonitorRect {
        MonitorRect {
            x,
            y,
            width,
            height,
        }
    }

    fn ids(rects: &[(String, MonitorRect)]) -> Vec<(String, ExtendViewport)> {
        group_viewports(rects).expect("group viewports")
    }

    #[test]
    fn empty_group_has_no_union() {
        assert_eq!(union_rect(&[]), None);
        assert!(group_viewports(&[]).is_none());
    }

    #[test]
    fn single_monitor_is_its_own_union() {
        let r = rect(0, 0, 1920, 1080);
        assert_eq!(union_rect(&[r]), Some(r));
        assert_eq!(
            extend_viewport(r, r),
            ExtendViewport {
                offset_x: 0,
                offset_y: 0,
                total_width: 1920,
                total_height: 1080,
                monitor_width: 1920,
                monitor_height: 1080,
            }
        );
    }

    #[test]
    fn primary_on_the_right_produces_negative_union_origin() {
        // 副屏在主屏左侧：虚拟桌面原点为 -1920
        let left = rect(-1920, 0, 1920, 1080);
        let right = rect(0, 0, 2560, 1440);
        let union = union_rect(&[left, right]).expect("union");
        assert_eq!(union, rect(-1920, 0, 4480, 1440));

        let a = extend_viewport(left, union);
        assert_eq!((a.offset_x, a.offset_y), (0, 0));
        let b = extend_viewport(right, union);
        assert_eq!((b.offset_x, b.offset_y), (1920, 0));
        assert_eq!((b.total_width, b.total_height), (4480, 1440));
    }

    #[test]
    fn vertical_offset_and_different_heights() {
        let top = rect(0, -1200, 1920, 1200);
        let bottom = rect(200, 0, 2560, 1440);
        let union = union_rect(&[top, bottom]).expect("union");
        assert_eq!(union, rect(0, -1200, 2760, 2640));

        let a = extend_viewport(top, union);
        assert_eq!((a.offset_x, a.offset_y), (0, 0));
        let b = extend_viewport(bottom, union);
        assert_eq!((b.offset_x, b.offset_y), (200, 1200));
    }

    #[test]
    fn group_viewports_keeps_ids_and_shares_one_union() {
        let monitors = vec![
            ("a".to_string(), rect(-1920, 0, 1920, 1080)),
            ("b".to_string(), rect(0, 0, 2560, 1440)),
        ];
        let viewports = ids(&monitors);
        assert_eq!(viewports.len(), 2);
        assert_eq!(viewports[0].0, "a");
        assert_eq!(viewports[1].0, "b");
        assert_eq!(viewports[0].1.offset_x, 0);
        assert_eq!(viewports[1].1.offset_x, 1920);
        assert_eq!(
            viewports[0].1.total_width, viewports[1].1.total_width,
            "整组必须共享同一个 union"
        );
    }

    #[test]
    fn offsets_never_exceed_union() {
        let rects = vec![rect(-500, -300, 1280, 720), rect(780, 420, 3840, 2160)];
        let union = union_rect(&rects).expect("union");
        for r in rects {
            let v = extend_viewport(r, union);
            assert!(v.offset_x >= 0 && v.offset_y >= 0);
            assert!(v.offset_x + v.monitor_width <= v.total_width);
            assert!(v.offset_y + v.monitor_height <= v.total_height);
        }
    }
}
