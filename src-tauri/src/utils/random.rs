//! 通用随机工具。
//!
//! 洗牌统一走 `fastrand::shuffle`，不自己实现 PRNG。
//!
//! 选 fastrand 2.4.1 而非 `rand 0.9` 的理由：前者已在本项目编译链里
//! （`tempfile → tauri-plugin-updater`），单 crate 零依赖，加直接依赖零新增
//! 编译单元；后者会额外拉进 rand_core / rand_chacha / getrandom / zerocopy
//! 等约 7 个 crate，只为一个洗牌不值当。
//!
//! 逻辑完备度没有打折（已核对 2.4.1 源码）：
//! - `Rng::shuffle` 是 Fisher-Yates 正向形式 `swap(i, usize(..=i))`；
//! - `gen_mod_u*` 用 Lemire multiply-high + 拒绝采样，**无取模偏差**；
//! - PRNG 为 WyRand（常量有命名与出处），全局函数走 std 熵初始化的线程局部 RNG，
//!   不用自己碰 `RandomState` 这类语义借用的种子来源。
//!
//! 洗牌会被多处复用（牌堆、未来的推荐 / 抽样），算法应只此一份。

/// 原地均匀洗牌；空切片与单元素切片是 no-op。
pub fn shuffle<T>(items: &mut [T]) {
    fastrand::shuffle(items);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_single_are_noop() {
        let mut empty: Vec<i32> = Vec::new();
        shuffle(&mut empty);
        assert!(empty.is_empty());

        let mut single = vec![7];
        shuffle(&mut single);
        assert_eq!(single, vec![7]);
    }

    #[test]
    fn shuffle_keeps_the_same_multiset() {
        let mut items = vec![1, 2, 3, 4, 5, 6, 7, 8];
        shuffle(&mut items);
        items.sort_unstable();
        assert_eq!(items, (1..=8).collect::<Vec<i32>>());
    }

    #[test]
    fn shuffle_works_for_non_integer_types() {
        let mut items = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        shuffle(&mut items);
        items.sort();
        assert_eq!(items, vec!["a", "b", "c"]);
    }
}
