# decode 层：解码 worker 与帧生产

## 本层职责

生产两类渲染产物：**帧容器**（图片/GIF）与**播放状态 Arc**（视频），供 render 层消费。不依赖 render 层。

## 模块

按格式分三组，每组一个子目录 + 门面 `mod.rs`：

| 子目录 | 文件 | 内容 |
|---|---|---|
| `image/` | `decode.rs` | 图片/GIF/动态 WebP 的纯解码与纹理上传函数（无状态，worker 的辅助模块） |
| | `worker.rs` | `ImageDecodeQueue`：一次性解码的调度、在途去重、generation 过期、结果回收（主体） |
| `video/` | `worker.rs` | 常驻视频 worker、播放状态、keyed mutex 共享纹理；内部 `VideoPlayback` 与 `VideoSession` 经 `WorkerCommand` 通信、耦合深，暂不拆文件 |
| `audio/` | `worker.rs` | 音轨解码与 WASAPI 输出（独立线程） |
| 根目录 | `frames.rs` | `GpuFrame` / `SharedFrames`：帧容器与帧时钟（三组共用） |

## 关键实现

### 两种 worker 模型

| | 图片 | 视频 |
|---|---|---|
| worker 形态 | 一次性线程，解码完即退出 | 常驻线程 + 消息循环 |
| 通信 | 单向 `decode_tx` → `poll()` | 双向 `WorkerCommand` + `SharedState` |
| 去重 | `DecodeKey(path,target)` + generation | `VideoIdentity(path,len,mtime)` 查已有 Arc |
| 状态 | 无状态，结果回收即结束 | 每路 `VideoPlayback` 带帧时钟/暂停/音量 |

选择依据：图片解码是幂等的数据变换，适合请求级去重；视频是长生命周期状态机，适合资源级复用。

### 在途去重的 generation 语义

`ImageDecodeQueue::request` 里 generation 递增**必须在合并判断之后**——否则 inflight 里存的永远是上一条命令的值，`filter(|gen| *gen == next_generation)` 恒 false（历史 bug，已修）。

结果回收时只删"仍指向自己"的 inflight 条目——A→B→A 时同 key 的新解码已覆盖 gen，旧结果不能误删新条目。

`poll()` 返回 `Vec<Ready>`，不碰 `surfaces`；过期结果与失败回执在队列内部消化。

### 帧时钟（`frames.rs`）

`SharedFrames { frames, started_at }`：

- 多屏共享同一个 `Arc`，`current_frame(now)` 按总时长取模选帧
- 各屏若各自 `Instant::now()` 会让 extend 接缝出现不同帧
- `reset_clock(now)` 在 attach 时对齐到真正挂载时刻，保证 GIF 从第一帧开始

### 视频的 keyed mutex 协议

#### 存在作用：跨线程 / 跨 device 传递帧所有权

视频帧有两个生产者/消费者，运行在**不同线程、不同 D3D11 device context** 上：

- **worker 线程**：DXVA 解码 + `VideoProcessorBlt` 写纹理（worker 自己的 device/context）
- **renderer 线程**：`CopyResource` 读纹理（SharedDevice 的 context）

D3D11 即时上下文非线程安全，且两边 device 不同（worker 的 DXVA device vs renderer 的 SharedDevice），无法用单 device 的 context 同步。**keyed mutex 是 D3D11 提供的跨 device / 跨线程同步原语**：`AcquireSync(key)` 阻塞直到该 key 可用，`ReleaseSync(key)` 把所有权交给对方。

注意：mutex 与屏数无关，**单屏同样必需**——它保护的是跨线程/跨 device 的传递，不是多屏之间的协调。多屏只需在 renderer 侧按 `Arc::as_ptr` 去重（见根 README 场景 3），保证每帧只复制一次、所有屏读同一本地副本。

```
key0 = worker 独占写
key1 = renderer 独占读

worker:    AcquireSync(0) → VideoProcessorBlt → Flush → ReleaseSync(1)
renderer:  begin_frame() → AcquireSync(1) → CopyResource → Flush → ReleaseSync(0)
           → 所有 surface 从 renderer 本地副本绘制
```

#### 实现目标

1. **正确性**：renderer 只读到完整帧，不是 Blt 写一半的纹理（否则撕裂/花屏）
2. **frame-level 去重**：同一 `VideoPlayback` 每帧只 `AcquireSync/CopyResource` 一次；mirror/extend 多屏共享同一本地副本——若每屏各抢一次 mutex，会出现"一块屏拿到帧、另一块屏拿到 key0 黑屏"
3. **隔离 DXVA 与渲染**：worker 用独立 device 做 Blt，不把 DXVA 操作塞进 renderer 线程

#### 历史教训：为什么 `VideoProcessorBlt` 不直接写 shared texture

原先做法是 Blt **直接写带 keyed mutex 的 shared texture**，实机报 `VIDEOPROCESSORBLT_OUTPUTHAZARD`（`0x80070057`）。

**根因**：`VideoProcessorBlt` 是 GPU 命令，可能被驱动切到另一个 GPU 上下文执行（多 GPU 机器尤其常见），而那个上下文**没有持有 keyed mutex**——带 mutex 的资源被未持锁的上下文写入，驱动判定为 unresolved hazard。

**修复**（commit `7027365`）：

```
原来：SourceReader → VideoProcessorBlt → shared keyed-mutex texture
修复：SourceReader → VideoProcessorBlt → worker 私有 staging texture（普通纹理，无 mutex）
                                        → CopyResource → shared texture（此时才用 mutex 传所有权）
```

关键洞察：**keyed mutex 只放在"跨线程传递"那一步**（CopyResource 的目标），不让复杂 GPU 操作（Blt）直接碰带 mutex 的资源。staging texture 按分辨率复用（`HashMap<(w,h), Texture>`），避免每 session 各持一份 NV12。

### 通信模型：worker ↔ renderer 到底耦合了什么

三类耦合性质完全不同，不要混为一谈：

| 耦合 | 载体 | 常态频率（30fps 视频） | 是不是高频消息 |
|---|---|---|---|
| 控制命令 | `mpsc<WorkerCommand>`：OpenSession / CloseSession / Pause / Reset / Shutdown + ack | 0 次/秒（只在状态切换时发，pause/volume 按值去重） | ❌ 事件驱动，播放中静默 |
| 帧时钟元数据 | `Arc<Mutex<SharedState>>`：`frame_time` / `frame_duration` / `loop_duration` / `generation` / `ready` | 30 次读+写/秒，**共享内存直接读写** | ❌ 无消息，只有一次 mutex lock + 几个标量拷贝 |
| 纹理数据 | shared NV12 texture + keyed mutex | 30 次 `CopyResource`/秒 | ✅ 每帧一次，但这是视频帧本身，已由 frame-level 去重压到每帧仅一次 |

worker 产出的帧**必须经过共享纹理**才能到 renderer，根源是三个句柄分属不同线程/device：

- worker 的独立 `ID3D11Device`（带 VIDEO_SUPPORT，供 DXVA）与 renderer 的 `SharedDevice` 是两个 device，**SRV 不能跨 device 绑定**
- `IMFDXGIDeviceManager` 的 device handle 由 worker 独占（`OpenDeviceHandle` / `CloseDeviceHandle`）
- swapchain / DirectComposition target 绑定在 renderer 线程 + 其 device 上，Present 只能在 renderer 线程

`GetSharedHandle` 的 NT handle + `OpenSharedResource` 是这两个 device 之间传帧的唯一合法交集。

此外还有一个架构层面的原因：composition 锚定在 renderer 线程的 HWND 消息泵、D3D11 context 与 60fps Present 节奏上，worker 侧又有 `ReadSample` / 首帧等待等阻塞操作，让 worker 持有合成树会导致窗口卡顿与 fade 失步。所以 worker 只产出帧、renderer 完成合成（详见根 README 场景 3）。

### 每帧纹理传输的代码定位

worker 写（`video/worker.rs`）：

| 位置 | 动作 |
|---|---|
| `VideoSession::process_sample`（`:1116`） | 每帧入口，由 `tick()` → `read_sample()` 后在 `:1376` 调用 |
| `:1155` `AcquireSync(0, 0)` | 取共享纹理写锁（取不到就跳过本帧，`return Ok(false)` 保留 sample 下轮重试） |
| `:1246` `CopyResource(&shared_texture, &output_texture)` | staging → shared texture 的实际传输 |
| `:1253` `ReleaseSync(release_key)` | 成功发 key1（renderer 可读），失败发 key0（worker 保留，供重试） |

renderer 读（`video/worker.rs` 里的 `VideoPlayback::begin_frame`，`:446`）：

| 位置 | 动作 |
|---|---|
| `:449` `state.ready` 检查 | 首帧 Blt 成功前不采样，避免全 0 NV12 解成绿屏 |
| `:455` `AcquireSync(1, 0)` | 取读锁，失败直接 return（不阻塞） |
| `:460` `CopyResource(&frame.local_texture, &frame.shared_texture)` | shared → renderer 本地副本的实际传输 |
| `:462` `ReleaseSync(0)` | 交还 worker |
| `:464` `frame_ready = true` | 此后 `frame_snapshot()` 才提供 Y/UV SRV |

调用链：

```text
renderer 主循环 :480 begin_video_frames(&surfaces)
    → 按 Arc::as_ptr 去重（:594），同一 VideoPlayback 每帧只调一次
    → VideoPlayback::begin_frame()（:446）
        → AcquireSync(1) → CopyResource → ReleaseSync(0)

worker tick()（:1376）
    → VideoSession::process_sample()（:1116）
        → AcquireSync(0) → Blt → CopyResource → ReleaseSync(1/0)
```

两个 `CopyResource` 一一对应：worker 那次把新帧放进共享纹理，renderer 那次把它取到本地副本。**每帧各一次，30fps 视频就是每秒 30 组**；多屏通过 `Arc` 指针去重不增加次数。

## 对外契约（只写输入输出）

| 输出 | 消费者 | 契约 |
|---|---|---|
| `SharedFrames` (`Arc`) | render 层 `WallpaperMedia::Frames` | 一次解码的帧序列；收到后由 render 调用 `reset_clock(now)` 对齐时钟 |
| `Ready` | renderer → `attach_media` | 含 frames / monitor_ids / fit_mode / crossfade / acks；renderer 负责挂屏与回执 |
| `Arc<Mutex<VideoPlayback>>` | render 层 `WallpaperMedia::Video` | 播放状态；Drop 时发 `CloseSession`，worker 释放 SourceReader/device |
| `mf_startup` / `mf_shutdown` | renderer | 进程级成对调用，必须在所有 worker 之前/之后 |

本层**不**关心：帧怎么画到屏幕、blend 怎么配、窗口怎么嵌入。
