//! 解码结果缓存（跨平台纯逻辑）
//!
//! 同一张壁纸会在三种场景里重复到达渲染线程：
//! 1. 首次设置壁纸：`update_wallpaper` 与 `syncWallpaperWindows → ensure_target` 各下发一次；
//! 2. mirror / extend：`upsertAll` 按显示器数并发 N 次命令，每次都整组下发；
//! 3. A → B → A 回切。
//!
//! 每条 `SetWallpaper` 都会触发全尺寸解码 + 逐帧纹理上传（4K JPEG 实测约 75ms，
//! 多帧 GIF 可达数百 MB），重复执行既拖慢切换又放大显存峰值。
//!
//! 缓存只存在于渲染线程：`SharedFrames` 持有 D3D11 SRV，不能跨线程共享；渲染线程
//! 单线程访问，无需锁。这里只实现与 GPU 无关 key / LRU / 字节预算，便于 macOS 单测，
//! 帧序列本身由 `FrameCache<T>` 泛型承载，Windows 侧传入 `SharedFrames`。

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// 缓存显存预算。Native renderer 和主进程同生命周期，缓存过大等价于把 WebView
/// 进程的解码常驻直接搬进主进程；这里压到 16MiB ≈ 2 张 1080p。
/// 4K 单帧约 33MiB、8K 单帧约 126MiB，超出预算不会入缓存，但仍由当前 surface
/// 的 Arc 持有，不影响当前显示，只影响 A→B→A 回切时的重解码成本。
pub const DEFAULT_MAX_BYTES: usize = 16 * 1024 * 1024;

/// 缓存值必须能报出自己占多少显存，用于字节预算淘汰。
pub trait ByteSize {
    fn byte_size(&self) -> usize;
}

/// 文件标识 = 路径 + 长度 + 修改时间。
///
/// 只按路径缓存会出问题：用户用同名文件覆盖壁纸（或从备份恢复）后仍会命中旧纹理。
/// 长度 + mtime 让覆盖自动失效，又不必读全文件算哈希。mtime 取不到时退化为 None，
/// 此时仍按路径缓存；文件本身异常时解码会失败，不会进入缓存。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CacheKey {
    path: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
    /// 解码目标尺寸。同一张图在不同屏配置（independent 单屏 / extend union）下
    /// 会解出不同分辨率的纹理，必须作为 key 的一部分，否则切换模式会命中错尺寸帧。
    /// None 用于不感知目标尺寸的调用方（如跨平台单测）。
    target: Option<(u32, u32)>,
}

/// 读取文件元数据生成 key；文件不存在 / 无权限时返回 None，调用方照常解码但不缓存。
pub fn cache_key(path: &Path) -> Option<CacheKey> {
    cache_key_with_target(path, None)
}

/// 带解码目标尺寸的 key：同一文件在不同目标分辨率下是不同的纹理。
pub fn cache_key_with_target(path: &Path, target: Option<(u32, u32)>) -> Option<CacheKey> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(CacheKey {
        path: path.to_path_buf(),
        len: metadata.len(),
        modified: metadata.modified().ok(),
        target,
    })
}

/// 帧序列缓存，泛型 `T` 由平台侧提供（Windows 为持有 D3D11 SRV 的 `SharedFrames`）。
/// 三个核心要素：`CacheKey` 判失效、`order` 维护 LRU、`bytes`/`max_bytes` 控制显存预算。
/// 只存在于渲染线程且单线程访问，故无需加锁。
pub struct FrameCache<T: ByteSize> {
    entries: HashMap<CacheKey, Arc<T>>,
    /// 最近使用在队尾；淘汰从队首开始
    order: VecDeque<CacheKey>,
    /// 当前已记账的字节数（不含被 surface 单独持有、已淘汰出缓存的份）。
    bytes: usize,
    /// 缓存的字节预算上限，来自 `DEFAULT_MAX_BYTES` 或自定义值。
    max_bytes: usize,
}

impl<T: ByteSize> FrameCache<T> {
    /// 以显存字节预算创建缓存；预算为 0 时所有条目都会因超预算而不入缓存。
    pub fn new(max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
            max_bytes,
        }
    }

    /// 使用默认预算创建缓存，便于各平台统一采用同一套显存上限。
    pub fn with_default_budget() -> Self {
        Self::new(DEFAULT_MAX_BYTES)
    }

    /// 只查缓存，不触发构造。异步解码链路用它把「查」和「存」拆到两个时机：
    /// 命令到达时查，worker 完成后存；命中时仍复用同一份 Arc（帧时钟一并复用）。
    pub fn get(&mut self, key: &CacheKey) -> Option<Arc<T>> {
        let frames = self.entries.get(key).cloned()?;
        self.touch(key);
        Some(frames)
    }

    /// 把异步解码完成的结果放入缓存；超预算 / key 异常时静默跳过，不影响显示。
    /// `get_or_insert_with` 保持同步调用方的既有语义，其内部继续复用本方法。
    pub fn insert(&mut self, key: CacheKey, value: Arc<T>) {
        self.insert_inner(key, value);
    }

    /// 命中则复用同一份 Arc（帧时钟也一并复用），未命中才执行 `make`。
    /// key 为 None 时完全绕过缓存，适用于文件元数据不可读的情况。
    pub fn get_or_insert_with<E>(
        &mut self,
        key: Option<CacheKey>,
        make: impl FnOnce() -> Result<Arc<T>, E>,
    ) -> Result<Arc<T>, E> {
        if let Some(ref key) = key {
            // 先 clone 再 touch：Arc 的 clone 很便宜，且能避免 entries 的
            // 不可变借用与 touch 的可变借用冲突。
            if let Some(frames) = self.entries.get(key).cloned() {
                self.touch(key);
                return Ok(frames);
            }
        }

        let frames = make()?;

        if let Some(key) = key {
            self.insert_inner(key, frames.clone());
        }
        Ok(frames)
    }

    /// 把命中的 key 移到队尾，标记为最近使用，使其在预算淘汰中最后被清除。
    fn touch(&mut self, key: &CacheKey) {
        if let Some(pos) = self.order.iter().position(|item| item == key) {
            self.order.remove(pos);
            self.order.push_back(key.clone());
        }
    }

    fn insert_inner(&mut self, key: CacheKey, frames: Arc<T>) {
        let size = frames.byte_size();
        // 单份就超预算时不入缓存：立刻把自己淘汰掉没有意义。
        // 多帧 GIF / 8K 素材属于这一类，交给 surface 的 Arc 自行持有。
        if size > self.max_bytes {
            return;
        }

        // 防守：若同 key 已存在（例如未来新增的下发路径绕过 get），先摘掉旧条目，
        // 避免 bytes 统计与 LRU 顺序里出现重复项。
        if let Some(old) = self.entries.remove(&key) {
            self.bytes = self.bytes.saturating_sub(old.byte_size());
            if let Some(pos) = self.order.iter().position(|item| *item == key) {
                self.order.remove(pos);
            }
        }

        while self.bytes + size > self.max_bytes {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(removed) = self.entries.remove(&oldest) {
                // 若该份仍被 surface 持有，GPU 纹理不会立刻释放；
                // 这里只从缓存预算里扣除，生命周期交给 Arc 自行收敛。
                self.bytes = self.bytes.saturating_sub(removed.byte_size());
            }
        }

        self.entries.insert(key.clone(), frames);
        self.bytes += size;
        self.order.push_back(key);
    }

    #[cfg(test)]
    fn cached_bytes(&self) -> usize {
        self.bytes
    }

    #[cfg(test)]
    fn contains(&self, key: &CacheKey) -> bool {
        self.entries.contains_key(key)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    struct Dummy(usize);

    impl ByteSize for Dummy {
        fn byte_size(&self) -> usize {
            self.0
        }
    }

    /// 临时文件 + Drop 清理守卫，避免测试遗留文件
    struct TempFile {
        path: PathBuf,
    }

    impl TempFile {
        fn new() -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let id = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "mini-wallpaper-cache-{}-{}-{}.bin",
                std::process::id(),
                nanos,
                id
            ));
            fs::write(&path, b"short").expect("create temp file");
            Self { path }
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    fn key(name: &str) -> CacheKey {
        CacheKey {
            path: PathBuf::from(name),
            len: 0,
            modified: None,
            target: None,
        }
    }

    #[test]
    fn hit_skips_decoder_and_reuses_same_arc() {
        let mut cache = FrameCache::<Dummy>::new(100);
        let calls = Cell::new(0);

        let first = cache
            .get_or_insert_with(Some(key("a")), || {
                calls.set(calls.get() + 1);
                Ok::<_, ()>(Arc::new(Dummy(10)))
            })
            .unwrap();
        let second = cache
            .get_or_insert_with(Some(key("a")), || {
                calls.set(calls.get() + 1);
                Ok::<_, ()>(Arc::new(Dummy(10)))
            })
            .unwrap();

        assert_eq!(calls.get(), 1, "命中缓存时不得再次解码");
        assert!(Arc::ptr_eq(&first, &second), "组内必须共享同一个帧时钟");
        assert_eq!(cache.cached_bytes(), 10);
    }

    #[test]
    fn none_key_bypasses_cache() {
        let mut cache = FrameCache::<Dummy>::new(100);
        let calls = Cell::new(0);

        for _ in 0..2 {
            cache
                .get_or_insert_with(None, || {
                    calls.set(calls.get() + 1);
                    Ok::<_, ()>(Arc::new(Dummy(10)))
                })
                .unwrap();
        }

        assert_eq!(calls.get(), 2);
        assert_eq!(cache.cached_bytes(), 0);
    }

    #[test]
    fn decoder_failure_does_not_pollute_cache() {
        let mut cache = FrameCache::<Dummy>::new(100);
        let result = cache.get_or_insert_with(Some(key("a")), || Err::<Arc<Dummy>, &str>("decode"));

        assert!(result.is_err());
        assert_eq!(cache.cached_bytes(), 0);
        assert!(!cache.contains(&key("a")));
    }

    #[test]
    fn eviction_respects_byte_budget() {
        let mut cache = FrameCache::<Dummy>::new(25);

        cache
            .get_or_insert_with(Some(key("a")), || Ok::<_, ()>(Arc::new(Dummy(10))))
            .unwrap();
        cache
            .get_or_insert_with(Some(key("b")), || Ok::<_, ()>(Arc::new(Dummy(10))))
            .unwrap();
        cache
            .get_or_insert_with(Some(key("c")), || Ok::<_, ()>(Arc::new(Dummy(10))))
            .unwrap();

        assert_eq!(cache.cached_bytes(), 20);
        assert!(!cache.contains(&key("a")), "最久未使用的 a 应被淘汰");
        assert!(cache.contains(&key("b")));
        assert!(cache.contains(&key("c")));
    }

    #[test]
    fn hit_moves_key_to_tail_before_eviction() {
        let mut cache = FrameCache::<Dummy>::new(25);

        cache
            .get_or_insert_with(Some(key("a")), || Ok::<_, ()>(Arc::new(Dummy(10))))
            .unwrap();
        cache
            .get_or_insert_with(Some(key("b")), || Ok::<_, ()>(Arc::new(Dummy(10))))
            .unwrap();
        // 命中 a，让它变成最近使用
        cache
            .get_or_insert_with(Some(key("a")), || -> Result<Arc<Dummy>, ()> {
                panic!("a 应该命中缓存")
            })
            .unwrap();
        cache
            .get_or_insert_with(Some(key("c")), || Ok::<_, ()>(Arc::new(Dummy(10))))
            .unwrap();

        assert!(cache.contains(&key("a")), "刚命中过的 a 不应被淘汰");
        assert!(!cache.contains(&key("b")), "应淘汰 b");
        assert!(cache.contains(&key("c")));
    }

    #[test]
    fn oversized_single_entry_is_not_cached() {
        let mut cache = FrameCache::<Dummy>::new(25);
        let frames = cache
            .get_or_insert_with(Some(key("8k")), || Ok::<_, ()>(Arc::new(Dummy(126))))
            .unwrap();

        assert_eq!(frames.byte_size(), 126, "超大素材仍要正常返回给调用方");
        assert_eq!(cache.cached_bytes(), 0);
        assert!(!cache.contains(&key("8k")));
    }

    #[test]
    fn cache_key_is_none_for_missing_file() {
        let path = std::env::temp_dir().join("mini-wallpaper-cache-definitely-missing.bin");
        assert!(cache_key(&path).is_none());
    }

    #[test]
    fn cache_key_changes_when_file_is_rewritten() {
        let temp = TempFile::new();
        let before = cache_key(&temp.path).expect("临时文件可读");

        fs::write(
            &temp.path,
            b"a much longer content, definitely a different length",
        )
        .expect("rewrite temp file");
        let after = cache_key(&temp.path).expect("临时文件仍可读");

        assert_ne!(before, after, "文件被覆盖后必须自动失效");
    }
}
