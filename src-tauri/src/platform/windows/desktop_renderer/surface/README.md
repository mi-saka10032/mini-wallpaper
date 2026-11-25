# surface 模块：壁纸渲染表面与共享 GPU 资源

## 1. 定位

Windows 原生壁纸渲染器的**资源层**：持有所有 D3D11 / DirectComposition 对象，负责"一张壁纸如何变成屏幕上的像素"。

它不决定**显示什么**（壁纸数据由上层 `UnifiedWallpaperManager` 决定）、不负责**视频解码**（`video.rs` 的常驻 worker）、不负责**图片解码调度**（`image_worker.rs` 的一次性 worker）。它接收已解码的帧或播放状态，把它们画到嵌入桌面的窗口上。

外部消费者只有三个文件，且引用路径在拆分前后保持不变：

```rust
renderer.rs     → surface::{RenderSurface, SharedDevice, WallpaperMedia}
image.rs        → surface::{GpuFrame, SharedFrames}
image_worker.rs → surface::SharedFrames
```

## 2. 架构分层

```text
                    ┌─────────────────┐
                    │  renderer.rs    │  主循环：命令分发、逐帧调度
                    └────────┬────────┘
                             │ 持有 SharedDevice + HashMap<monitor_id, RenderSurface>
        ┌────────────────────┼────────────────────┐
        ▼                    ▼                    ▼
┌───────────────┐   ┌────────────────┐   ┌────────────────┐
│  image.rs     │   │  image_worker  │   │   video.rs     │
│ 纯解码+上传   │   │ 调度/去重/回收 │   │ 常驻 worker    │
└───────┬───────┘   └────────┬───────┘   └────────┬───────┘
        │ SharedFrames       │ Ready              │ Arc<Mutex<VideoPlayback>>
        └────────────────────┼────────────────────┘
                             ▼
┌────────────────────────────────────────────────────────────┐
│                      surface 模块                          │
│                                                            │
│  frames.rs      GpuFrame / SharedFrames（帧时钟）           │
│  pipeline.rs    HLSL + 9 个管线对象（一次创建，全局共享）   │
│  device.rs      SharedDevice（device / context / pipeline） │
│  composition.rs swapchain + DirectComposition 合成链       │
│  render_surface RenderSurface（三层内容 + 绘制）            │
│  device_lost.rs 设备丢失判定（跨环节复用）                  │
└────────────────────────────────────────────────────────────┘
```

**核心分层原则**："共享层"与"单屏层"分离。

- `SharedDevice` 全局唯一：device / context / pipeline / video worker 只有一份。extend 组内所有 surface 必须绑定同一份纹理与管线，否则接缝会出现不同帧或不同混合结果
- `RenderSurface` 每屏一份：自有窗口、swapchain、RTV、三层内容状态；从共享层借 device / context / pipeline

## 3. 各模块职责

### `mod.rs`（22 行）

模块声明 + `pub(crate) use` re-export。拆分时"对外零改动"的关键：外部仍写 `surface::RenderSurface`，不知道内部有 6 个文件。

只 re-export 三个外部需要的类型：`SharedDevice` / `SharedFrames` / `GpuFrame` / `RenderSurface` / `WallpaperMedia`。`QuadPipeline` / `FadeConstants` / `Vertex` 只在模块内跨文件使用，不对外暴露。

### `frames.rs`（67 行）

帧数据与帧时钟，是**解码侧与渲染侧的交汇点**：

- `GpuFrame`：一张已上传 GPU 的帧，`view` 是 SRV（内部持有纹理引用），`duration` 供动图取模选帧
- `SharedFrames`：帧序列 + `started_at` 时钟。多屏共享同一个 `Arc`，`current_frame(now)` 按总时长取模选帧——各屏若各自 `Instant::now()` 会让 extend 接缝出现不同帧

### `pipeline.rs`（367 行）

全屏 quad 的绘制管线。**HLSL 源码、字节对齐结构、9 个 D3D 对象**集中在此：

- `SHADER_SOURCE`：`vs_main`（共用）+ `ps_main`（BGRA 图片）+ `ps_nv12`（NV12 视频，BT.601 转换）。三者共用同一个 `cbuffer Fade`
- `Vertex` / `FadeConstants`：`#[repr(C)]`，与 HLSL 的 `VS_IN` / `cbuffer` 逐字节对齐
- `QuadPipeline`：9 个字段对应 D3D11 固定管线的 9 个状态槽（VS、两个 PS、光栅化、输入布局、顶点缓冲、采样器、blend、fade cbuffer），无冗余
- `create_pipeline`：一次创建全部对象，由 `SharedDevice::new()` 调用一次并 clone 给所有 surface

### `device.rs`（80 行）

`SharedDevice`：渲染线程独有的共享 D3D11 上下文 + video worker 宿主。

- `device`：D3D11 设备，供 image worker 在渲染线程外上传纹理（必须 clone 同一个，SRV 不能跨 device 用）
- `context`：即时上下文，非线程安全，只在渲染线程使用
- `pipeline`：上面 9 个管线对象
- `video_worker`：单个常驻 worker 服务全部 `VideoPlayback`，每路播放自带独立 GPU / SourceReader

### `composition.rs`（100 行）

DirectComposition 合成链：`swapchain → visual → target → 壁纸 HWND`。

`CompositionSwapChain` 的 4 个 COM 对象必须一起存活（字段只持有引用，不直接读取），任一 drop 会让 visual 失效、桌面合成树取不到内容。

swapchain 的关键配置：`BGRA8_UNORM` + `FLIP_SEQUENTIAL` + `PREMULTIPLIED` alpha——这是 24H2 桌面合成树正确采样壁纸窗口的前提（详见 `NATIVE_RENDERER_ARCHITECTURE_REVIEW.md`）。

### `render_surface.rs`（595 行）

单屏壁纸渲染状态与绘制，模块内最大的一块。不进一步拆分的原因：它是一个高度内聚的状态机，拆成多个 `impl` 文件需要把 16 个字段和约 10 个方法改成 `pub(super)`，可见性成本 > 可读性收益。

方法按功能连续排列：

```text
构造与销毁    new / destroy / Drop
内容状态机    queue_wallpaper → start_next_transition → finish_transition
              fade_active_or_pending / needs_continuous_frames
              video / video_arcs / clear_wallpaper
配置与几何    set_extend_viewport / set_fit_mode / update_rect
绘制准备      bind_current_backbuffer / has_drawable_frame
绘制          render / draw_media
```

**三层内容状态**是核心设计：

| 层 | 字段 | 作用 |
|---|---|---|
| 当前 | `wallpaper: Option<WallpaperMedia>` | fade 期间仍是 from |
| 过渡 | `transition: Option<Transition>` | fade 的 to，结束后提升为 wallpaper |
| 排队 | `pending: VecDeque<PendingWallpaper>` | 只保留最新一项（每个视频带独立 worker/device，FIFO 堆积会暴涨内存） |

视频的暂停/音量/冻结必须覆盖全部三层，靠 `video_arcs()` 汇总——这是"切屏不漏音、fade 中不提前出声"的关键。

### `device_lost.rs`（41 行）

D3D11 / DXGI 设备丢失判定。独立成模块因为它与具体 surface / device 状态无关：

- `DeviceLostHResult` trait：从 anyhow 包装链或 windows 原生错误中提取 HRESULT
- `is_device_lost()`：判定 `DEVICE_REMOVED` / `DEVICE_RESET` / `DRIVER_INTERNAL_ERROR`

`render_surface.rs` 的 `render()` 在绑定 RTV 和 Present 两处调用它；返回 `true` 时上层必须清理整组 surface 与共享 device，不能只重试单屏。

## 4. 数据流

### 图片链路

```text
image.rs::decode_and_upload (一次性 worker)
    → SharedFrames (帧序列 + 时钟)
    → image_worker.rs::poll() 返回 Ready
    → renderer.rs::attach_media(...) 
    → RenderSurface::queue_wallpaper(WallpaperMedia::Frames(Arc<SharedFrames>))
    → draw_media: ps_main 采样 BGRA 纹理 × fade_alpha
```

组内多屏共享同一个 `Arc<SharedFrames>`：一条 `SetWallpaper` 带 `monitor_ids`，一次解码的帧 clone 给每屏，帧时钟天然一致。

### 视频链路

```text
video.rs 常驻 worker: MF SourceReader → DXVA VideoProcessorBlt → shared NV12 texture
    → Arc<Mutex<VideoPlayback>> 
    → renderer.rs::find_reusable_video (同身份复用，不开新 worker)
    → RenderSurface::queue_wallpaper(WallpaperMedia::Video(Arc))
    → draw_media: ps_nv12 采样 Y/UV 双平面 + BT.601 转 RGB × fade_alpha
```

`draw_media` 的分流判据极简：`views[1].is_some()`——有第二张纹理（UV 平面）就用 `ps_nv12`，否则用 `ps_main`。图片帧被包成 `VideoFrameSnapshot`（`uv_view = None`）以复用后续绘制路径。

### 一帧的绘制序列（每屏）

```text
bind_current_backbuffer  (GetBuffer(0) → CreateRenderTargetView，尺寸变化时 ResizeBuffers)
    → ClearRenderTargetView  (有壁纸但无帧时清 [0,0,0,0] 让系统壁纸透出)
    → draw_media(from, alpha=1.0)   [fade 时]
    → draw_media(to, alpha=progress) [fade 时]
    → swapchain.Present(1, 0)
```

`draw_media` 内部 11 条 set 调用依次绑定 blend → fade cbuffer → input layout → vertex buffer → VS → PS → sampler → SRV → rasterizer → viewport → `Draw(4, 0)`。`QuadPipeline` 的 9 个字段正是这些调用所需的对象集合。

## 5. 边界控制

### 可见性

拆分后保持"对外零改动"：

- `mod.rs` re-export 5 个外部类型，外部引用路径不变
- 跨模块的私有项按最小必要放宽：
  - `pub(crate)`：`GpuFrame` / `SharedFrames` / `RenderSurface` / `WallpaperMedia` / `SharedDevice`
  - `pub(super)`：`QuadPipeline` / `Vertex` / `FadeConstants` / `create_pipeline` / `CompositionSwapChain` / `create_composition_swap_chain` / `is_device_lost` / pipeline 各字段
- `SharedDevice` / `CompositionSwapChain` 的字段为 `pub(crate)`：`RenderSurface` 跨文件读取

### 生命周期

```text
RenderSurface::drop → destroy()
    → context.OMSetRenderTargets(None) + render_target = None   显式解绑 RTV
    → wallpaper / transition / pending 清空
        → Arc<VideoPlayback> drop → CloseSession → worker 释放 SourceReader / device
    → unembed_from_desktop（若在 embedded 状态）
    → NativeWindow::drop → DestroyWindow
    → COM 字段按声明顺序自动 Release
```

`swap_chain` 字段先于 `composition_*` drop，但 `SetContent` 内部已 AddRef，不会悬空。

### 错误传播

- **设备丢失**：`is_device_lost()` 在 RTV 绑定和 Present 两处探测；返回 `true` 让上层清理整组，不在本模块内重试
- **普通错误抑制**：`backbuffer_error_logged` / `present_error_logged` 各自独立、成功时复位，避免每帧刷屏又不丢失后续故障
- **shader 编译失败**：`compile_shader` 把 D3DCompile 的错误 blob 拼进 anyhow，定位到 HLSL 行号

## 6. 已知边界与待办

| 项 | 状态 |
|---|---|
| `ps_nv12` 动态颜色矩阵 / range | **待办**：写死 BT.601 + limited range，720p+ 多为 709。元数据已在 `video.rs` 读取但仅用于日志。详见 `docs/NATIVE_RENDERER_ARCHITECTURE_REVIEW.md` §12 |
| gamma 域 alpha blend | 接受：2D 合成通行做法，改线性域需全链路 sRGB 格式 + shader 回编码，收益低 |
| `destroy()` 不显式释放 swapchain | 接受：唯一调用方是 `Drop`，无复用场景；每屏固定一份，非内存上涨根因 |
| `device` 字段只被用 1 次 | 接受：建 RTV 必须用 device，context 无法替代 |

## 7. 验证约定

`surface` 是 Windows-only 代码，本地 `cargo check` 不编译它。改动后必须：

1. 本地 `cargo fmt --check` + `cargo test --lib`（验证跨平台逻辑未受影响）
2. Windows target probe：**先删除 probe 里的 surface 副本，再从真实目录全量重拷**，然后 `cargo check --target x86_64-pc-windows-msvc`

第 2 步的"全量重拷"是硬性要求——曾因复用旧副本掩盖过一次字段类型被误改的污染。
