# desktop_renderer：系统设备链路 ↔ 代码映射

这份文档回答一个问题：**"某个运行时场景下，Windows 设备做了什么、代码走到哪里"**。

各层的技术实现细节见各层目录自己的 README；这里只做**场景 → 模块/函数**的对账表。

## 目录定位

```text
desktop_renderer/
├── renderer.rs      入口 / 编排（跨三层协调，不属于任何单一层）
├── device/          设备层：COM、HWND、设备丢失判定
├── decode/          解码 worker 层：image/ video/ audio/ 三组 + frames.rs
└── render/          渲染层：帧的消费与呈现
```

依赖方向单向：`device ← render → decode`，入口 `renderer.rs` 协调三层。

---

## 场景 1：应用启动 / renderer 线程建立

**系统设备侧发生了什么**：COM 运行时在当前线程初始化为 STA；D3D11 设备与即时上下文创建；窗口类注册、壁纸窗口创建并嵌入桌面合成树；MF 平台启动。

| 顺序 | 模块::函数 | 设备动作 |
|---|---|---|
| 1 | `renderer::run` → `device::ComScope::init` | `CoInitializeEx(APARTMENTTHREADED)` |
| 2 | `renderer::run` → `decode::mf_startup` | `MFStartup`（进程级，一次） |
| 3 | `renderer::run` → `render::SharedDevice::new` | `D3D11CreateDevice` |
| 4 | `render::SharedDevice::new` → `render::pipeline::create_pipeline` | 编译 3 套 HLSL + 创建 9 个管线对象 |
| 5 | `render::SharedDevice::new` → `decode::VideoWorkerHost::start` | spawn 常驻视频 worker 线程 |
| 6 | 每个屏 `render::RenderSurface::new` → `device::NativeWindow::new` | `CreateWindowEx` + `WS_EX_NOREDIRECTIONBITMAP` |
| 7 | `render::RenderSurface::new` → `desktop_embedder::embed_in_desktop` | `SetParent` 到 Progman/WorkerW |
| 8 | `render::RenderSurface::new` → `render::composition::create_composition_swap_chain` | DXGI swapchain + DirectComposition visual → HWND |

**关键约束**：步骤 1 必须在 2 之前（MFStartup 要求 COM 已初始化）；步骤 6 的窗口样式必须在创建期声明，事后补无效。

---

## 场景 2：设置一张静态图片

**设备侧**：turbojpeg 在 CPU 解码 → D3D11 纹理上传 → GPU 每帧把纹理贴到全屏 quad → Present 交给桌面合成器。

| 顺序 | 模块::函数 | 设备动作 |
|---|---|---|
| 1 | `decode::decode_and_upload`（image/decode.rs） | JPEG/BMP/GIF 解码为 BGRA 帧序列 |
| 2 | 同上 | `CreateTexture2D` + `CreateShaderResourceView` 上传 GPU |
| 3 | `decode::ImageDecodeQueue::request`（image/worker.rs） | spawn 一次性解码 worker；同 key 在途去重 |
| 4 | `decode::ImageDecodeQueue::poll`（image/worker.rs） → `renderer::attach_media` | 结果回收，`reset_clock(now)` 对齐帧时钟 |
| 5 | `render::RenderSurface::queue_wallpaper(Frames(Arc))` | 写入三层内容状态 |
| 6 | 每帧 `render::RenderSurface::render` → `draw_media` | `ps_main` 采样纹理 × fade_alpha |
| 7 | `swap_chain.Present(1, 0)` | 提交给桌面合成树 |

**多屏场景**：mirror/extend 时一条命令带 `monitor_ids`，一次解码的 `Arc<SharedFrames>` clone 给组内每屏，帧时钟天然一致。

---

## 场景 3：设置视频壁纸

**设备侧**：MF SourceReader 打开流 → DXVA device manager 分配解码设备 → keyed mutex 在 worker/renderer 间传递共享纹理 → `VideoProcessorBlt` 输出到 staging NV12 → 每帧 GPU 做 YUV→RGB。

| 顺序 | 模块::函数 | 设备动作 |
|---|---|---|
| 1 | `renderer` 视频分支 → `decode::VideoIdentity::from_path` | 记录 path + len + mtime |
| 2 | `renderer::find_reusable_video` | 遍历 surface 三层找同身份 `Arc`，命中则不开新 worker |
| 3 | `decode::VideoWorkerHost::start` / `VideoPlayback::open` | MF `SourceReader` + `IMFDXGIDeviceManager` |
| 4 | `decode::video::worker::VideoSession::process_sample` | `VideoProcessorBlt` → NV12 staging texture |
| 5 | `CopyResource` → shared texture，`ReleaseSync(1)` | keyed mutex 交给 renderer |
| 6 | `render::RenderSurface::draw_media` | `ps_nv12` 采样 Y/UV + BT.601 转 RGB |
| 7 | `decode::AudioPlayback` | 音轨存在时独立线程走 WASAPI 输出 |

**关键约束**：worker/renderer 必须按 keyed mutex 协议交替持有共享纹理，否则 `OUTPUTHAZARD`。

#### 多屏共享机制

keyed mutex **单屏也必需**——它保护的是跨线程 + 跨 device 的纹理传递，与屏数无关：worker 用独立 DXVA device 写、renderer 用 SharedDevice 读，即使只有一块屏也要交替持锁。多屏时的额外机制是 **frame-level 去重**：

| | 单屏 | mirror / extend 多屏 |
|---|---|---|
| `VideoPlayback` 实例 | 1 个 | **1 个**（N 个 Arc 指向同一实例，由 `find_reusable_video` 复用） |
| `begin_frame()` / `AcquireSync(1)` | 每帧 1 次 | **每帧 1 次**（不是 N 次） |
| 本地副本 | 1 份 | **1 份**，N 块屏都从它绘制 |
| 设备动作 | worker Blt → CopyResource → N/A | 同样一次 Blt + 一次 CopyResource，然后所有 surface 读同一本地纹理 |

关键在 `renderer::begin_video_frames` 按 `Arc::as_ptr` 去重：遍历所有 surface 的 `video_arcs()`，同一 `Arc` 只调一次 `begin_frame()`。若不去重，第二块屏的 `AcquireSync(1, 0)` 会失败（key1 已被第一块屏 `ReleaseSync(0)` 交出），导致"一块屏有画面、其它屏黑屏"。

independent 模式下每路视频是**不同**的 Arc，不去重、各复制一次——这是正确行为，因为它们本就是不同纹理。

#### 为什么 worker 不能持有 composition（共享纹理存在的根本原因）

常见误解是"DirectComposition 无法跨线程"，不准确——它是 COM 对象，按规则可在别的线程创建。真正不可跨的是它**锚定的三样东西**：

| 约束 | 为什么锁在 renderer 线程 |
|---|---|
| HWND 消息泵 | 窗口在哪个线程创建，WndProc 就在哪个线程执行、消息队列线程局部。`pump_messages()` 必须与 `NativeWindow::new()` 同线程 |
| D3D11 即时上下文 | 非线程安全；所有 surface 绘制共用 renderer 的 context |
| Present 节奏 | 60fps 出帧 + 所有 surface 统一裁决，不能被解码阻塞 |

composition 的 `SetRoot` / `Commit` 只是这三者的胶水，跟着它们走。

若让 worker 持有 composition + HWND，有三个具体后果：

1. **窗口卡顿**：worker 有阻塞操作——`ReadSample` 同步拉帧、首帧等待最多 10 秒（`wait_deadline` + `sleep(2ms)` 循环）、空闲时 sleep。窗口消息和 Present 排在后面，拖动/DPI 变化延迟，首帧慢的视频可让窗口 10 秒无响应
2. **fade 失步**：crossfade 裁决天然在 renderer——`sync_video_pause` 冻结 from/to 两侧时钟、`finish_transition` 提升 to 并按 Arc 去重 reset、`crossfade_progress` 按统一 `now` 算权重。合成树在 worker 就要跨线程同步"谁是 from/to、进度多少"，极易两侧不同步
3. **多屏共享破坏**：`find_reusable_video` 让 mirror/extend 的 N 屏指向同一个 `VideoPlayback`，`begin_video_frames` 按 Arc 去重保证每帧只 Copy 一次；合成树分散到 worker 后这个共享语义不成立

还有一条：图片和视频**共用同一套 surface**（`WallpaperMedia::Frames | Video`）。composition 进 worker 要么让图片也跟着走（推翻"图片一次性 worker"设计），要么维护两套合成树 + 跨线程 fade——都比现在复杂。

**结论**：worker 只能产出帧数据，通过共享纹理交给 renderer 完成合成。这不是 API 限制，是架构约束——排查类似问题时不要往"composition 能不能跨线程"方向找。

---

## 场景 4：crossfade 切换

**设备侧**：两 pass 绘制，预乘 alpha 混合，产出 `to·t + from·(1-t)`。

| 顺序 | 模块::函数 | 设备动作 |
|---|---|---|
| 1 | `render::RenderSurface::queue_wallpaper(crossfade=true)` | 新内容进 `transition`/`pending` |
| 2 | `renderer::sync_video_pause` | fade 期间冻结 from/to 两侧视频时钟 |
| 3 | 每帧 `draw_media(from, alpha=1.0)` | pass1 写满 RT |
| 4 | 每帧 `draw_media(to, alpha=progress)` | pass2 用 blend state 混合 |
| 5 | `render::RenderSurface::finish_transition` | fade 结束后 to 提升为 wallpaper |

**混合数学**：`render::pipeline` 的 blend state 设 `ONE / INV_SRC_ALPHA`，shader 输出预乘色。`FadeConstants` cbuffer（`b0`）每帧写入 alpha。

---

## 场景 5：设备丢失

**设备侧**：DXGI 返回 `DEVICE_REMOVED` / `DEVICE_RESET`，整块 device 上所有资源失效。

| 顺序 | 模块::函数 | 设备动作 |
|---|---|---|
| 1 | `render::RenderSurface::render` 在 RTV 绑定或 Present 处调 `device::is_device_lost` | 识别错误 HRESULT |
| 2 | 返回 `true` → renderer DeviceLost 分支 | 清理整组 surface + SharedDevice |
| 3 | `RenderSurface::destroy` → `unembed_from_desktop` | 恢复窗口父关系 |
| 4 | `render::frames` 缓存丢弃 / `decode::ImageDecodeQueue::invalidate` | 作废旧解码结果 |
| 5 | `on_device_lost()` 回调 → `ctx::unified_manager::errors::device_lost_handler` | emit 事件 + 清 WebView 窗口 |
| 6 | 下一次命令 `renderer::ensure_shared` | 惰性重建整条设备链 |

**关键约束**：不能只重试单屏——设备丢失时所有 surface 的 SRV 同时失效。

---

## 场景 6：窗口移动 / 分辨率变化

| 顺序 | 模块::函数 | 设备动作 |
|---|---|---|
| 1 | `render::RenderSurface::update_rect` | 比对 rect，变化时继续 |
| 2 | `device::NativeWindow::set_bounds` | `SetWindowPos` |
| 3 | `swap_chain.ResizeBuffers` | 重建 backbuffer（先释放所有引用） |
| 4 | `bind_current_backbuffer` | `GetBuffer(0)` → `CreateRenderTargetView` |
| 5 | `refresh_group_geometry` → `set_group_geometry` | extend 组重算视口 |

---

## 场景 7：音量调节 / 全屏暂停

| 顺序 | 模块::函数 | 设备动作 |
|---|---|---|
| 1 | `renderer::sync_audio_state` | 遍历所有 surface 的 `video_arcs()` |
| 2 | `decode::AudioPlayback::set_volume` | 每路视频音量（按 Arc 去重避免组内重复设置） |
| 3 | `renderer::sync_video_pause` | 全屏 / fade 暂停按 reason 合并，冻结视频时钟 |

---

## 优化待办：按视频编码标准切换 YUV→RGB 矩阵

**状态**：待办，架构重组完成后单独处理。本轮（三层目录拆分）不实施。

### 问题

`render/pipeline.rs:64` 的 `ps_nv12` 写死 **BT.601 + limited range** 系数（BT.601 权重 `0.299/0.587/0.114` 经 `255/219`、`255/224` 缩放）。但矩阵由**视频文件**决定，不是显示器：720p/1080p 多标 BT.709，4K/8K 可能是 BT.2020，录屏可能是 full range。用错矩阵产生色度偏移（灰度不变、肤色差 6~9/255、高饱和色差 15~39/255），非几何失真，壁纸场景影响有限但真实存在。

D3D11 `VideoProcessorBlt` 输入输出均为 NV12（同格式缩放，不做 YUV→RGB），因此矩阵转换 100% 发生在 `ps_nv12`——这正是需要修的位置。

### 已有基础设施

`decode/video/worker.rs:1103-1105` 的 `media_type_diag()` 已在读取但**仅用于日志**：

- `MF_MT_YUV_MATRIX`（`:1103`）
- `MF_MT_VIDEO_NOMINAL_RANGE`（`:1105`）

### 落点（按当前三层结构）

| 层 | 文件 / 位置 | 改动 |
|---|---|---|
| decode | `video/worker.rs` 的 `media_type_diag` / `SharedFrameInfo` | 读 matrix + range（从仅日志改为驱动行为），经 `SharedFrameInfo` 传给 renderer |
| render | `pipeline.rs` 的 `SHADER_SOURCE` | 新增 `ps_nv12_709`（BT.709 权重 `0.2126/0.7152/0.0722`）；range 用 cbuffer 参数或再拆变体 |
| render | `render_surface.rs::draw_media` | 按帧携带的 matrix 选 pixel shader（当前判据是 `views[1].is_some()`，需扩展） |

缺失元数据时按分辨率猜：`< 720` 行默认 601，`≥ 720` 行默认 709（ffmpeg/mpv/浏览器通行启发式）；无法判断维持 601。

### 不推荐的方案

- 只改 shader 常量 → 把偏色换到另一边
- 让 `VideoProcessorBlt` 输出 RGBA → staging 从 NV12 变 BGRA8，显存涨约 60%

量化数据与历史背景见 `docs/NATIVE_RENDERER_ARCHITECTURE_REVIEW.md` §12。

---

## 各层技术文档

- `device/README.md` —— COM 生命周期、HWND 嵌入、设备丢失判定
- `decode/README.md` —— 图片/视频 worker 模型、帧容器、去重策略、通信模型
- `render/README.md` —— 资源聚合、三层内容状态、绘制路径
