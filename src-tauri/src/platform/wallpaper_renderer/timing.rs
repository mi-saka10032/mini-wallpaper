//! 视频帧节拍的跨平台纯逻辑。
//!
//! Windows-only 的 decode worker 消费这里的门控函数；抽到跨平台模块是为了让
//! 时序逻辑（历史多次出 pacing bug）能在 macOS 开发机上直接单测，不必依赖
//! Windows 目标或 MF / D3D11 运行时。

use std::time::Duration;

/// 墙钟未超过上一帧结束时间时跳过本轮，避免解码快于展示。
///
/// `target` 与 `frame_time` 必须处在**同一圈内**的同一条时间轴上；每圈 EOF 会同时
/// 归零二者（重置 worker 的 `started` 与 `frame_time`）。若只归零 `frame_time` 而
/// `target` 继续单调，此函数会恒返回 true，表现为「循环后高倍速」——单测里的
/// 反例固定住这个约束。
pub fn should_read_next_frame(
    target: Duration,
    frame_time: Duration,
    frame_duration: Duration,
    ready: bool,
) -> bool {
    !(ready && target < frame_time.saturating_add(frame_duration))
}

/// 一圈长度：优先本圈真实帧结束时间，容器元数据兜底，最后才是墙钟。
///
/// 不用估计值累加——D 的偏差会随圈数线性放大，让门控长期偏斜。
pub fn resolve_loop_duration(
    cycle_last_end: Duration,
    presentation_duration: Duration,
    target: Duration,
) -> Duration {
    if !cycle_last_end.is_zero() {
        cycle_last_end
    } else if !presentation_duration.is_zero() {
        presentation_duration
    } else {
        target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_millis(33);

    #[test]
    fn gate_waits_until_current_frame_ends() {
        assert!(!should_read_next_frame(
            Duration::from_millis(10),
            Duration::ZERO,
            FRAME,
            true
        ));
        assert!(should_read_next_frame(
            Duration::from_millis(33),
            Duration::ZERO,
            FRAME,
            true
        ));
    }

    #[test]
    fn gate_always_reads_first_frame() {
        // ready=false 时不设门控，否则首帧永远出不来。
        assert!(should_read_next_frame(
            Duration::ZERO,
            Duration::ZERO,
            FRAME,
            false
        ));
    }

    #[test]
    fn per_cycle_timing_does_not_diverge() {
        // 每圈同时归零 target 与 frame_time：第 1 圈与第 100 圈的首帧门控结果一致，
        // 误差无法跨圈累积（本次修复的核心不变量）。
        let first = should_read_next_frame(Duration::ZERO, Duration::ZERO, FRAME, true);
        for _ in 0..100 {
            assert_eq!(
                should_read_next_frame(Duration::ZERO, Duration::ZERO, FRAME, true),
                first
            );
        }
    }

    #[test]
    fn gate_would_race_if_only_frame_time_were_reset() {
        // 反例保护：只归零 frame_time、target 继续单调（旧实现的隐患），
        // 门控恒 true → worker 全速追赶。修复必须连 started 一起重置。
        assert!(should_read_next_frame(
            Duration::from_secs(10),
            Duration::ZERO,
            FRAME,
            true
        ));
    }

    #[test]
    fn gate_is_saturating_near_duration_limit() {
        // 极端值不 panic：saturating_add 吸收溢出。
        assert!(should_read_next_frame(
            Duration::MAX,
            Duration::MAX,
            FRAME,
            true
        ));
    }

    #[test]
    fn loop_duration_prefers_real_frames_then_container_then_wall_clock() {
        let real = Duration::from_secs(10);
        let container = Duration::from_millis(9800);
        let wall = Duration::from_millis(10_050);
        assert_eq!(resolve_loop_duration(real, container, wall), real);
        assert_eq!(
            resolve_loop_duration(Duration::ZERO, container, wall),
            container
        );
        assert_eq!(
            resolve_loop_duration(Duration::ZERO, Duration::ZERO, wall),
            wall
        );
    }
}
