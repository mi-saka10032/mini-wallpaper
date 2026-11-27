//! 随机播放牌堆：环形洗牌队列的纯内存状态。
//!
//! ## 为什么只在内存里
//!
//! 牌堆是「运行时播放状态」，不是收藏夹的成员顺序。命中集由规则实时求值、
//! 不物化，一旦落库，规则编辑 / 成员增删 / 软删除之后库里的 id 立刻变成
//! 孤儿行，生命周期没有自然终点，还要在所有写路径做联动清理。纯内存则
//! 只需在失配时重洗，零残留、零 migration、备份导入也不会带进失效 id。
//!
//! ## 为什么是环形
//!
//! 乱序牌堆本身没有头尾：Prev 从队首绕到队尾、Next 从队尾绕回队首。
//! 不设「耗尽重洗」，一轮内每张恰好出现一次，来去都在同一条确定序列上，
//! 与音乐播放器的 shuffle 列表语义一致（上一首再下一首能回到原处）。
//!
//! ## 为什么不用逐处埋重洗钩子
//!
//! 每次消费前重新求值命中集，与队列里的快照比对；再校验当前显示 id 是否
//! 等于 cursor 指向的那张。任何导入 / 删除 / 恢复 / 规则编辑 / 标签变更 /
//! 手动设图 / 删除联动造成的变化都会自动触发「以当前 id 为锚重洗」，
//! 新增写路径无需埋点，也不会因漏配而让新成员永远抽不到。

use std::collections::HashMap;

use crate::dto::shortcut_dto::Direction;
use crate::utils::random::shuffle;

/// 单块显示器（同步组共用一个 key）的播放序列。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayQueue {
    /// 队列属于哪个收藏夹；换绑后必须重洗，不能沿用旧夹的序列。
    collection_id: i32,
    /// 生成序列时的命中集快照（升序去重），用作失效判据。
    hit_ids: Vec<i32>,
    /// `[anchor] + shuffle(命中集 \ {anchor})`；anchor 不在命中集时就是全量洗牌。
    sequence: Vec<i32>,
    /// 当前显示的图在 sequence 中的位置；None = 当前图不在序列里，Next 从 0 起。
    cursor: Option<usize>,
}

impl PlayQueue {
    fn empty(collection_id: i32) -> Self {
        Self {
            collection_id,
            hit_ids: Vec::new(),
            sequence: Vec::new(),
            cursor: None,
        }
    }

    /// cursor 指向的那张；与 DB 的 wallpaper_id 不一致说明发生了外部改写。
    fn current_id(&self) -> Option<i32> {
        self.cursor.and_then(|i| self.sequence.get(i).copied())
    }

    /// 是否必须以当前显示图为锚重建序列。
    ///
    /// 三条判据覆盖所有失配：换夹、命中集变化、当前显示图被外部改写。
    /// 正常轮播 / B1 消费后 DB 写入的正是 cursor 那张，不会误判。
    ///
    /// cursor=None 是「当前显示图不在序列里」的合法状态（夹外图 / 已软删），
    /// 此时 current_id() 恒为 None，若照常比较会导致每 tick 都重洗、序列永不
    /// 稳定。这种情况下只在锚点重新落回命中集内时才重洗（从夹外切回夹内）。
    fn needs_reseed(&self, collection_id: i32, hit_ids: &[i32], anchor: Option<i32>) -> bool {
        let anchor_changed = match self.cursor {
            Some(_) => self.current_id() != anchor,
            None => anchor.is_some_and(|id| hit_ids.contains(&id)),
        };
        self.collection_id != collection_id || self.hit_ids.as_slice() != hit_ids || anchor_changed
    }

    /// 以 anchor 为锚重洗：锚点排在第 0 位、其余洗牌跟随，保证一轮全覆盖
    /// 且锚点不会立刻重复出现。
    fn reseed(&mut self, collection_id: i32, mut hit_ids: Vec<i32>, anchor: Option<i32>) {
        hit_ids.sort_unstable();
        hit_ids.dedup();

        let mut rest = hit_ids.clone();
        if let Some(a) = anchor {
            if let Some(pos) = rest.iter().position(|&id| id == a) {
                rest.remove(pos);
                shuffle(&mut rest);
                let mut sequence = Vec::with_capacity(rest.len() + 1);
                sequence.push(a);
                sequence.extend(rest);
                self.sequence = sequence;
                self.cursor = Some(0);
                self.collection_id = collection_id;
                self.hit_ids = hit_ids;
                return;
            }
        }

        // 锚点为空 / 不在命中集（夹外图、已软删）：当前显示图不在序列里，
        // cursor 置 None，由 Next 从队首开始、Prev 绕到队尾。
        shuffle(&mut rest);
        self.sequence = rest;
        self.cursor = None;
        self.collection_id = collection_id;
        self.hit_ids = hit_ids;
    }

    fn next(&mut self) -> Option<i32> {
        let len = self.sequence.len();
        if len == 0 {
            return None;
        }
        let i = match self.cursor {
            None => 0,
            Some(i) => (i + 1) % len,
        };
        self.cursor = Some(i);
        Some(self.sequence[i])
    }

    fn prev(&mut self) -> Option<i32> {
        let len = self.sequence.len();
        if len == 0 {
            return None;
        }
        let i = match self.cursor {
            Some(0) | None => len - 1,
            Some(i) => i - 1,
        };
        self.cursor = Some(i);
        Some(self.sequence[i])
    }

    #[cfg(test)]
    fn with_sequence(
        collection_id: i32,
        hit_ids: Vec<i32>,
        sequence: Vec<i32>,
        cursor: Option<usize>,
    ) -> Self {
        Self {
            collection_id,
            hit_ids,
            sequence,
            cursor,
        }
    }
}

/// 所有显示器的牌堆集合。
///
/// 用 `std::sync::Mutex` 而非 tokio 的：临界区只有 Vec 洗牌与 cursor 移动，
/// 零 await，同步锁还能让 `AppEffects::stop` 这种同步方法顺手清条目，
/// 避免显示器热插拔后 HashMap 只增不删。
#[derive(Debug, Default)]
pub struct PlayQueueStore {
    queues: HashMap<String, PlayQueue>,
}

impl PlayQueueStore {
    /// 选出下一张 / 上一张；必要时先按当前锚点重洗。
    ///
    /// `hit_ids` 由调用方在锁外求值后传入，保证临界区内没有 DB await。
    pub fn pick(
        &mut self,
        key: &str,
        collection_id: i32,
        hit_ids: &[i32],
        anchor: Option<i32>,
        direction: &Direction,
    ) -> Option<i32> {
        let queue = self
            .queues
            .entry(key.to_string())
            .or_insert_with(|| PlayQueue::empty(collection_id));
        if queue.needs_reseed(collection_id, hit_ids, anchor) {
            queue.reseed(collection_id, hit_ids.to_vec(), anchor);
        }
        match direction {
            Direction::Next => queue.next(),
            Direction::Prev => queue.prev(),
        }
    }

    /// timer 停止 / 显示器消失 / 解绑轮播时删条目。
    pub fn remove(&mut self, key: &str) {
        self.queues.remove(key);
    }

    /// display_mode 切换、备份导入等整库级变化时清空。
    pub fn clear(&mut self) {
        self.queues.clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.queues.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reseed_puts_anchor_first_and_covers_every_id_once() {
        let mut q = PlayQueue::empty(7);
        q.reseed(7, vec![3, 1, 2, 4], Some(2));
        assert_eq!(q.sequence[0], 2);
        assert_eq!(q.sequence.len(), 4);
        let mut sorted = q.sequence.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![1, 2, 3, 4]);
        assert_eq!(q.cursor, Some(0));
        assert_eq!(q.current_id(), Some(2));
    }

    #[test]
    fn ring_next_wraps_from_tail_to_head() {
        let mut q = PlayQueue::with_sequence(1, vec![1, 2, 3], vec![1, 2, 3], Some(2));
        assert_eq!(q.next(), Some(1));
        assert_eq!(q.current_id(), Some(1));
    }

    #[test]
    fn ring_prev_wraps_from_head_to_tail() {
        let mut q = PlayQueue::with_sequence(1, vec![1, 2, 3], vec![1, 2, 3], Some(0));
        assert_eq!(q.prev(), Some(3));
        assert_eq!(q.current_id(), Some(3));
    }

    #[test]
    fn prev_then_next_returns_to_the_same_card() {
        let mut q = PlayQueue::with_sequence(1, vec![1, 2, 3], vec![1, 2, 3], Some(1));
        assert_eq!(q.prev(), Some(1));
        assert_eq!(q.next(), Some(2));
    }

    #[test]
    fn cursor_none_starts_next_at_head_and_prev_at_tail() {
        let mut q = PlayQueue::empty(1);
        q.reseed(1, vec![1, 2, 3], Some(99));
        assert_eq!(q.cursor, None);
        let tail = q.sequence.last().copied();
        assert_eq!(q.prev(), tail);
        let mut q2 = PlayQueue::empty(1);
        q2.reseed(1, vec![1, 2, 3], Some(99));
        let head = q2.sequence[0];
        assert_eq!(q2.next(), Some(head));
    }

    #[test]
    fn out_of_set_anchor_keeps_queue_stable_until_anchor_returns() {
        let ids = vec![1, 2, 3];
        // cursor=None：当前显示图（夹外图 99）不在序列里。
        let q = PlayQueue::with_sequence(1, ids.clone(), vec![3, 1, 2], None);
        assert!(!q.needs_reseed(1, &ids, Some(99)));
        assert!(!q.needs_reseed(1, &ids, None));
        // 命中集变化仍必须重洗，否则新成员永远进不来。
        assert!(q.needs_reseed(1, &[1, 2, 3, 4], Some(99)));
        // 锚点切回夹内 → 以该图为锚重洗。
        assert!(q.needs_reseed(1, &ids, Some(2)));
    }

    #[test]
    fn empty_hit_set_yields_no_card() {
        let mut q = PlayQueue::empty(1);
        q.reseed(1, vec![], Some(5));
        assert_eq!(q.next(), None);
        assert_eq!(q.prev(), None);
    }

    #[test]
    fn single_member_queue_stays_on_that_card() {
        let mut q = PlayQueue::empty(1);
        q.reseed(1, vec![7], Some(7));
        assert_eq!(q.next(), Some(7));
        assert_eq!(q.prev(), Some(7));
    }

    #[test]
    fn hit_set_change_and_external_switch_both_trigger_reseed() {
        let mut q = PlayQueue::with_sequence(1, vec![1, 2], vec![1, 2], Some(0));
        assert!(!q.needs_reseed(1, &[1, 2], Some(1)));
        assert!(q.needs_reseed(1, &[1, 2, 3], Some(1))); // 导入 / 恢复 / 规则放宽
        assert!(q.needs_reseed(2, &[1, 2], Some(1))); // 换绑收藏夹
        assert!(q.needs_reseed(1, &[1, 2], Some(99))); // 手动设图 / 删除联动
    }

    #[test]
    fn normal_consumption_does_not_trigger_reseed() {
        let mut store = PlayQueueStore::default();
        let ids = vec![1, 2, 3];
        let first = store.pick("carousel:a", 1, &ids, Some(1), &Direction::Next);
        assert!(first.is_some());
        // 写库成功后 DB 的 wallpaper_id 就是刚选出的那张，不应重洗。
        let q = store.queues.get("carousel:a").unwrap();
        assert_eq!(q.current_id(), first);
        assert!(!q.needs_reseed(1, &ids, first));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn store_remove_and_clear_drop_entries() {
        let mut store = PlayQueueStore::default();
        let ids = vec![1, 2];
        store.pick("carousel:a", 1, &ids, Some(1), &Direction::Next);
        store.pick("carousel:b", 1, &ids, Some(2), &Direction::Next);
        assert_eq!(store.len(), 2);
        store.remove("carousel:a");
        assert_eq!(store.len(), 1);
        store.clear();
        assert_eq!(store.len(), 0);
    }
}
