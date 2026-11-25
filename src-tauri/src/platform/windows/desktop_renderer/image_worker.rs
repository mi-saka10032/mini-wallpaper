//! 图片解码的调度与生命周期（一次性 worker）
//!
//! 与 `video.rs` 的常驻 worker 不同：图片解码是无状态的一次性工作，`image.rs`
//! 只提供纯函数 `decode_and_upload`。本模块负责它外面那一层「调度」：
//! 在途去重、generation 过期、结果回收与 ack 回执。
//!
//! 之前这些状态（4 个可变变量 + 3 个私有类型）散落在 `renderer.rs` 主循环里，
//! 既难读也容易把 generation 递增放错位置。现在收成 `ImageDecodeQueue`：
//! renderer 只调 `request` / `poll` / `invalidate` / `shutdown` 四个方法。
//!
//! **本模块不碰 `surfaces`**：`poll()` 返回 `Ready`，由 renderer 决定如何挂屏，
//! 保持「解码调度」与「渲染状态」解耦。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Instant;

use anyhow::Result;
use windows::Win32::Graphics::Direct3D11::ID3D11Device;

use super::image;
use super::surface::SharedFrames;
use super::{send_acks, Ack};
use crate::platform::wallpaper_renderer::FitMode;

/// 标识"同一张图 + 同一解码目标尺寸"，仅用于合并**在途**的重复解码请求。
///
/// 不做缓存：图片重复解码代价可接受，而缓存要维护 LRU / 显存预算 / 失效策略。
/// 这里只保留去重所需的最小信息；文件覆盖由 generation 过期机制处理。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct DecodeKey {
    path: PathBuf,
    target: (u32, u32),
}

/// 在途的一次性图片解码：worker 完成后结果经 `decode_tx` 回到渲染线程。
/// 只活到结果回收；过期 generation 的结果被丢弃，ack 仍回 Ok 避免误报 error。
struct PendingDecode {
    key: DecodeKey,
    monitor_ids: Vec<String>,
    fit_mode: FitMode,
    crossfade: bool,
    /// 同 key 的重复命令会合并到同一次解码，所有等待方都要收到回执。
    acks: Vec<Ack>,
    handle: JoinHandle<()>,
}

/// worker → 渲染线程的解码结果。`generation` 用于连点时丢弃过期结果。
struct DecodeResult {
    generation: u64,
    result: std::result::Result<SharedFrames, String>,
}

/// `poll()` 产出的「已解码、可挂屏」结果。
///
/// renderer 拿到后自行 `attach_media`；`primary_ack` 随挂载结果回，
/// 其余等待方由 renderer 统一回 Ok（保持原有的回执语义）。
pub(super) struct Ready {
    pub frames: Arc<SharedFrames>,
    pub monitor_ids: Vec<String>,
    pub fit_mode: FitMode,
    pub crossfade: bool,
    pub now: Instant,
    pub primary_ack: Option<Ack>,
    pub other_acks: Vec<Ack>,
}

/// 图片解码队列：聚合在途状态，供渲染线程串行调用。
pub(super) struct ImageDecodeQueue {
    decode_tx: Sender<DecodeResult>,
    decode_rx: Receiver<DecodeResult>,
    pending: HashMap<u64, PendingDecode>,
    /// key → 在途 generation：同 key 的重复命令合并到同一次解码。
    inflight: HashMap<DecodeKey, u64>,
    next_generation: u64,
}

impl ImageDecodeQueue {
    pub fn new() -> Self {
        let (decode_tx, decode_rx) = mpsc::channel::<DecodeResult>();
        Self {
            decode_tx,
            decode_rx,
            pending: HashMap::new(),
            inflight: HashMap::new(),
            next_generation: 0,
        }
    }

    /// 让所有在途解码结果过期：清屏 / 销毁显示器 / 切视频 / DeviceLost 时调用。
    pub fn invalidate(&mut self) {
        self.next_generation += 1;
    }

    /// 请求一次解码。返回 `Ok(())` 表示已 spawn 或已合并到在途解码；
    /// `Err` 仅表示 worker 线程创建失败，调用方应把 ack 回给等待方。
    /// 设备是否可用由 renderer 的 `ensure_shared` 负责，不进本模块。
    pub fn request(
        &mut self,
        device: &ID3D11Device,
        path: PathBuf,
        target_width: u32,
        target_height: u32,
        monitor_ids: Vec<String>,
        fit_mode: FitMode,
        crossfade: bool,
        ack: Option<Ack>,
    ) -> Result<()> {
        let key = DecodeKey {
            path: path.clone(),
            target: (target_width, target_height),
        };

        // 只认「当前 generation」：若中间已有别的换图命令递增过 generation，
        // 那条在途解码的结果注定会被判过期丢弃，合并它没有意义。
        if let Some(inflight_gen) = self
            .inflight
            .get(&key)
            .copied()
            .filter(|gen| *gen == self.next_generation)
        {
            if let Some(pending) = self.pending.get_mut(&inflight_gen) {
                for monitor_id in monitor_ids {
                    if !pending.monitor_ids.contains(&monitor_id) {
                        pending.monitor_ids.push(monitor_id);
                    }
                }
                if let Some(ack) = ack {
                    pending.acks.push(ack);
                }
                return Ok(());
            }
        }

        // 确认无法合并才递增：连点 / 清屏 / 销毁显示器时，让所有更早发出的
        // 在途解码结果自动过期。递增必须放在合并判断之后，否则 inflight 里
        // 存的永远是上一条命令的 generation，去重恒不生效。
        self.next_generation += 1;
        let generation = self.next_generation;

        let worker_device = device.clone();
        let worker_target = Some((target_width, target_height));
        let tx = self.decode_tx.clone();
        let handle = thread::Builder::new()
            .name("wallpaper-image-decode".to_string())
            .spawn(move || {
                let result = image::decode_and_upload(&worker_device, &path, worker_target)
                    .map_err(|err| format!("{:#}", err));
                let _ = tx.send(DecodeResult { generation, result });
            })?;

        self.inflight.insert(key.clone(), generation);
        self.pending.insert(
            generation,
            PendingDecode {
                key,
                monitor_ids,
                fit_mode,
                crossfade,
                acks: ack.into_iter().collect(),
                handle,
            },
        );
        Ok(())
    }

    /// 回收已完成解码；返回所有仍有效、可挂屏的结果。
    ///
    /// 过期结果与解码失败都在本方法内部消化（回执按原语义处理），不会产出 `Ready`。
    pub fn poll(&mut self) -> Vec<Ready> {
        let mut ready = Vec::new();
        while let Ok(DecodeResult { generation, result }) = self.decode_rx.try_recv() {
            let Some(pending) = self.pending.remove(&generation) else {
                continue;
            };
            // worker 已发回结果，join 只做线程资源回收，不会阻塞。
            let _ = pending.handle.join();
            // 只删「仍指向自己」的条目：A→B→A 时同 key 的新解码已覆盖 gen，
            // 旧结果回收不能顺手删掉新条目的去重索引。
            if self.inflight.get(&pending.key) == Some(&generation) {
                self.inflight.remove(&pending.key);
            }

            if generation != self.next_generation {
                log::info!(
                    "[ImageWorker] 解码结果过期丢弃 generation={} latest={}",
                    generation,
                    self.next_generation
                );
                send_acks(pending.acks, Ok(()));
                continue;
            }

            match result {
                Ok(frames) => {
                    // 帧时钟对齐到真正 attach 的时刻，保证 GIF 从第一帧开始、
                    // 组内所有屏共享同一条时间线。
                    let now = Instant::now();
                    let mut frames = frames;
                    frames.reset_clock(now);
                    // attach 只回一个 ack：它是「内容已挂载」的语义代表，
                    // 其余等待方统一回 Ok。
                    let mut acks = pending.acks;
                    let primary_ack = acks.pop();
                    ready.push(Ready {
                        frames: Arc::new(frames),
                        monitor_ids: pending.monitor_ids,
                        fit_mode: pending.fit_mode,
                        crossfade: pending.crossfade,
                        now,
                        primary_ack,
                        other_acks: acks,
                    });
                }
                Err(message) => {
                    log::error!("[ImageWorker] {}", message);
                    send_acks(pending.acks, Err(message));
                }
            }
        }
        ready
    }

    /// 渲染线程退出时收尾：join 所有在途 worker，回 Err 避免协调器永久挂住。
    pub fn shutdown(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        for (_, pending) in pending {
            let _ = pending.handle.join();
            send_acks(
                pending.acks,
                Err("渲染线程退出，图片解码未完成".to_string()),
            );
        }
        self.inflight.clear();
        // 所有 worker 已 join，channel 随本类型 Drop 自然释放；无需额外断链。
    }
}

impl Drop for ImageDecodeQueue {
    fn drop(&mut self) {
        self.shutdown();
    }
}
