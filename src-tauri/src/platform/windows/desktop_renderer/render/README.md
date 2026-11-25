# render 层：帧消费与呈现

## 本层职责

持有 GPU 资源与每屏状态，把 decode 层产出的帧画到嵌入桌面的窗口上，并 Present 给桌面合成树。依赖下层 device（窗口 / COM / 错误判定）与 decode（帧容器 / 播放状态）。

## 模块

| 文件 | 内容 |
|---|---|
| `shared_device.rs` | `SharedDevice`：本层资源聚合根（device / context / pipeline / video worker） |
| `pipeline.rs` | HLSL 源码、`#[repr(C)]` 对齐结构、9 个绘制管线对象 |
| `composition.rs` | DXGI swapchain + DirectComposition 合成链 |
| `render_surface.rs` | `RenderSurface`：单屏三层内容状态与绘制 |
| `device_lost.rs` | *(已移至 device 层，本目录无此文件)* |

## 关键实现

### SharedDevice 是资源聚合根

`device / context / pipeline / video_worker` 全局只有一份：SRV 不能跨 device 使用，extend 组所有 surface 必须绑定同一份纹理与管线。

`device` / `context` 为 `pub(crate)` 字段，供 render_surface 创建 RTV 与下发绘制命令；`pipeline` 由 `create_pipeline` 一次创建并 clone 给各 surface。

### QuadPipeline 的 9 个字段

对应 D3D11 固定管线的 9 个状态槽，无冗余：vertex shader / 两个 pixel shader / rasterizer / input layout / vertex buffer / sampler / blend / fade cbuffer。

两个 pixel shader 的分流判据是 `views[1].is_some()`——有 UV 平面（视频）用 `ps_nv12`，否则（图片）用 `ps_main`。两者共用 `cbuffer Fade`，故 crossfade 不需要第二套绘制路径。

### RenderSurface 三层内容状态

| 层 | 字段 | 作用 |
|---|---|---|
| 当前 | `wallpaper` | fade 期间仍是 from |
| 过渡 | `transition.to` | fade 目标，结束后提升为 wallpaper |
| 排队 | `pending` | 只保留最新一项（每个视频带独立 GPU，FIFO 堆积会暴涨内存） |

`video_arcs()` 汇总三层的视频 Arc，供音量设置 / fade 冻结 / 帧推进统一裁决——这是"切屏不漏音、fade 中不提前出声"的关键。

`destroy()` 显式解绑 RTV → 清三层内容 → unembed（幂等）；`NativeWindow::Drop` 负责销毁 HWND。

### 呈现配置

swapchain 用 `BGRA8_UNORM` + `FLIP_SEQUENTIAL` + `PREMULTIPLIED` alpha；shader 输出预乘色，blend 用 `ONE / INV_SRC_ALPHA`，两 pass fade 的 alpha 收敛到 1。这是 24H2 桌面合成树正确采样壁纸窗口的前提。

## 对外契约（只写输入输出）

| 输入来源 | 输入 | 本层动作 |
|---|---|---|
| decode 层 | `Arc<SharedFrames>` | 存入 `WallpaperMedia::Frames`，`ps_main` 绘制 |
| decode 层 | `Arc<Mutex<VideoPlayback>>` | 存入 `WallpaperMedia::Video`，`ps_nv12` 绘制 |
| renderer | `queue_wallpaper(...)` | 写入三层状态 |
| renderer | `render(monitor_id, now)` | 返回 `true` 表示设备丢失，调用方必须整组清理 |

| 输出 | 契约 |
|---|---|
| `RenderSurface` / `WallpaperMedia` / `SharedDevice` | 由层门面 re-export 给 renderer 与外部 |
| `Present` 结果 | 失败时按 `device::is_device_lost` 分流；普通错误只记一次日志 |

本层**不**关心：图片/视频怎么解码、worker 怎么调度、COM 怎么初始化。
