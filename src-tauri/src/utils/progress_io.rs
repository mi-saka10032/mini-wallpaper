//! IO 进度追踪工具
//!
//! 提供 `ProgressWriter`，通过包装底层 `Write` 实现字节级精确进度回调，
//! 适用于 zip 打包/解压等 IO 密集场景。
//!
//! ## 设计要点
//! - 共享计数器：`ProgressCounter` 基于 `Arc<AtomicU64>`，天然支持 clone 共享，
//!   单文件独享或多文件共享同一进度均通过同一个 `ProgressWriter` 实现
//! - 零开销可选回调：`on_progress` 为 `Option`，无回调时 advance 仅做原子累加，不产生任何调用开销
//! - 回调节流：默认每写入 64KB 触发一次回调，避免高频调用
//! - 泛型透传：对底层 IO trait（Write / Seek）做最小化包装，不影响原有语义

use std::io::{self, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// 节流阈值：每累计写入 64KB 触发一次进度回调
const THROTTLE_BYTES: u64 = 64 * 1024;

/// 字节级进度回调类型：(已处理字节数, 总字节数)
pub type ByteProgressFn = Box<dyn Fn(u64, u64) + Send + Sync>;

/// 共享的字节计数器
///
/// 基于 `Arc` 实现 clone 共享，调用方按需决定使用方式：
/// - **独享**：构造后传入单个 `ProgressWriter`，追踪单文件写入进度
/// - **共享**：clone 后传入多个 `ProgressWriter`，累计跨文件的全局字节进度
/// - **无回调**：传入 `None`，advance 仅做原子累加，零额外开销
#[derive(Clone)]
pub struct ProgressCounter {
    written: Arc<AtomicU64>,
    total: u64,
    last_reported: Arc<AtomicU64>,
    on_progress: Option<Arc<dyn Fn(u64, u64) + Send + Sync>>,
}

impl ProgressCounter {
    /// 创建进度计数器
    ///
    /// `on_progress` 为 `None` 时，advance 仅累加字节数，不触发任何回调
    pub fn new(total: u64, on_progress: Option<ByteProgressFn>) -> Self {
        Self {
            written: Arc::new(AtomicU64::new(0)),
            total,
            last_reported: Arc::new(AtomicU64::new(0)),
            on_progress: on_progress.map(|cb| Arc::from(cb) as Arc<dyn Fn(u64, u64) + Send + Sync>),
        }
    }

    /// 累加字节数，仅在有回调且超出节流阈值时触发
    fn advance(&self, n: u64) {
        let current = self.written.fetch_add(n, Ordering::Relaxed) + n;

        if let Some(ref cb) = self.on_progress {
            let last = self.last_reported.load(Ordering::Relaxed);
            if current - last >= THROTTLE_BYTES || current >= self.total {
                self.last_reported.store(current, Ordering::Relaxed);
                cb(current, self.total);
            }
        }
    }
}

/// 带进度追踪的 Writer 包装器
///
/// 拦截每次 `write` 调用，通过 `ProgressCounter` 累计字节数并按需触发回调。
/// 同时透传 `Seek` trait，满足 `ZipWriter<W: Write + Seek>` 等场景的约束。
///
/// ## 使用示例
///
/// ```rust,ignore
/// // 导出：单文件独享计数器（带回调）
/// let counter = ProgressCounter::new(total, Some(callback));
/// let writer = ProgressWriter::new(file, counter);
/// let mut zip = ZipWriter::new(writer);
///
/// // 导入：多文件共享计数器
/// let counter = ProgressCounter::new(total, Some(callback));
/// for entry in archive {
///     let writer = ProgressWriter::new(out_file, counter.clone());
///     io::copy(&mut entry, &mut writer)?;
/// }
///
/// // 无回调：纯 IO 透传，零额外开销
/// let counter = ProgressCounter::new(total, None);
/// let writer = ProgressWriter::new(file, counter);
/// ```
pub struct ProgressWriter<W: Write> {
    inner: W,
    counter: ProgressCounter,
}

impl<W: Write> ProgressWriter<W> {
    /// 创建 ProgressWriter，绑定指定的进度计数器
    pub fn new(inner: W, counter: ProgressCounter) -> Self {
        Self { inner, counter }
    }

    /// 消费 wrapper，返回底层 Writer（预留接口，供需要获取底层 Writer 的场景使用）
    #[allow(dead_code)]
    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: Write> Write for ProgressWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.counter.advance(n as u64);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// 透传 Seek trait，使 `ZipWriter<ProgressWriter<File>>` 满足 `Write + Seek` 约束
impl<W: Write + Seek> Seek for ProgressWriter<W> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}
