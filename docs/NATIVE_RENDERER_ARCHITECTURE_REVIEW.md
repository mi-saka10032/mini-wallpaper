# Native 壁纸渲染架构评审

- 日期：2026-11-22
- 分支：`feature/native-wallpaper-renderer`
- 当前验证基线：代码 `6f831d5`，文档 HEAD 随分支（session 级 GPU + fade 期间视频/音频暂停 + 音频线程 paused 初始状态修复）
- 评审对象：Windows Native 壁纸渲染链路，重点为视频解码、多屏绘制、内存生命周期与后续架构方向

---

## 1. 结论摘要

当前 Native 方案已经完成从“验证可行性”到“工程稳定”的转变：

1. **视频硬解链路成立**
   - 独立 DXVA worker + Media Foundation SourceReader + D3D11 VideoProcessor + shared texture 链路已跑通。
   - CPU 从软件解码约 15% 降到约 1%。

2. **多屏闪烁已解决**
   - mirror / extend 下双屏视频稳定显示。
   - keyed mutex 从 per-surface 改为 per-render-frame，所有 surface 只读同一份 renderer 本地纹理副本。

3. **生命周期泄漏已收敛**
   - 每帧 `VideoProcessorInputView` 已显式释放。
   - DXGI device manager handle 已关闭。
   - 连续切换视频时 pending 队列只保留最新项。
   - 相同视频身份复用已有 `VideoPlayback`。
   - `MFStartup` / `MFShutdown` 已配对。
   - 长时间播放、切换、回落的内存趋于稳定。

4. **内存优势按媒体类型分化**
   - 纯图片壁纸：Native 有明确内存优势；
   - 视频壁纸：单视频时 Native 已不再占内存优势，独立视频窗口越多优势越弱；
   - 最终采用 session 级 GPU，是因为它把每个视频的 MF/DXVA/GPU cache holder 绑定到 `VideoSession`，切换后可整体释放，且工程管理更清晰；
   - 全局单 GPU 在多屏场景有更低的边际内存：首视频 cache 拉高后稳定，第二个视频只小幅上升；但该方案会让旧 cache 跨视频长期留存，最终未采用；
   - 两轮 A/B 已验证：全局单 GPU 不会消除首个视频的 cache 峰值，session 级 GPU 的固定成本更高但生命周期更干净；
   - Native 视频方案的真实优势是低 CPU、帧同步可控、少一个 WebView renderer，不是最低稳态内存。

5. **全局 Worker + session 级 GPU 已实施并验收**
   - `VideoWorkerHost` 管理唯一 video worker 线程，所有 session 串行处理；
   - 每个 `VideoSession` 持有自己的 `WorkerGpu` / D3D11 device / manager / staging texture；
   - `VideoPlayback` 退化为 renderer 侧 handle；
   - `CloseSession` 时先释放 SourceReader / view / shared texture，再关闭 device manager；
   - 保持切换稳定性，同时避免全局 GPU device/manager 在视频链路持续完整推进时长期持有 MF/DXVA cache；并避免 debug layer 的生命周期跟踪在该长生命周期 device 上挂住已释放 D3D11 对象；
   - independent / mirror / extend、切换、暂停、循环均已实机验收通过。

6. **下一步的收益判断**
   - 继续找 Windows / COM / D3D11 / MF 的 “clear cache” API 没有实际意义。
   - Native 解码拆到子进程主要改善 main 进程账面和隔离性，不会降低总内存，且增加 IPC/句柄复杂度，不建议作为内存优化首选。
   - 当前不建议立即实现 Worker 线程真正退出/按需重启：GPU 资源在 session 关闭时即释放，空闲线程成本极低，线程重启会引入 MF/命令通道竞态。

---

## 2. 背景

项目存在两套壁纸渲染路径：

| 方案 | 渲染位置 | 优点 | 问题 |
|---|---|---|---|
| WebView | WebView2 renderer | 实现简单，浏览器生态成熟 | 多 WebView renderer、内存峰值高、CPU/GPU 成本较高 |
| Native | Rust 主进程 + D3D11 + MF | CPU 极低、峰值低、进程更少 | main 进程常驻变高，视频缓存迁移进 main |

用户关注的核心场景：

```text
60MB 2K 视频主屏 + 图片壁纸副屏
```

对比目标不是单纯 main 进程数字，而是：

```text
总内存
峰值内存
CPU
GPU
进程数量
长期稳定性
```

---

## 3. 当前 Native 架构

### 3.1 线程与对象

当前架构包含：

```text
Renderer 线程
  唯一
  负责 surface、swapchain、D3D11 绘制、Present

VideoPlayback
  每个视频一个
  renderer 侧持有 Arc<Mutex<VideoPlayback>>

DXVA worker 线程
  每个 VideoPlayback 一个
  负责 SourceReader、Media Foundation、DXVA、VideoProcessor

Audio 线程
  每个 VideoPlayback 一个
  当前默认音频失败忽略，不作为当前内存归因
```

注意：

```text
渲染线程是唯一 worker 线程不是唯一
当前每个视频都有自己的 worker 和 video device
```

### 3.2 视频帧链路

当前视频链路：

```text
MF SourceReader
→ DXVA decoder
→ IMFDXGIBuffer
→ decoder texture
→ VideoProcessorBlt
→ shared NV12 texture
→ keyed mutex
→ renderer CopyResource
→ renderer local NV12 texture
→ Y/UV SRV
→ ps_nv12 shader
→ quad draw
```

### 3.3 keyed mutex 协议

```text
key0：worker 可写
key1：renderer 可读
```

流程：

```text
worker:
  AcquireSync(0)
  VideoProcessorBlt
  Flush
  ReleaseSync(1)

renderer:
  begin_frame()
  AcquireSync(1)
  CopyResource
  Flush
  ReleaseSync(0)
  all surfaces draw from local texture
```

frame-level 去重后：

```text
同一 VideoPlayback 每帧只 AcquireSync / CopyResource 一次
mirror / extend 多屏共享同一本地纹理
independent 多视频各自复制一次
```

---

## 4. 已验证事实

### 4.1 功能验证

用户实机结论：

1. mirror 稳定显示；
2. extend 稳定显示；
3. 闪烁消失；
4. 切换正常；
5. CPU 约 1% 或更低；
6. 长时间播放不持续上涨；
7. 切换视频后可回落。

### 4.2 内存对比

#### 早期 Native

```text
2K 20MB 视频循环：main 约 120MB
2K 60MB 视频循环：main 约 200MB
5 个视频循环：130MB → 160MB 后封顶
```

#### 当前总内存对比

场景：

```text
60MB 2K 视频主屏 + 图片壁纸副屏
```

| 方案 | 稳定状态 | 峰值 |
|---|---:|---:|
| WebView | main 40MB + WebView 270MB ≈ 310MB | WebView 短暂 370MB，总约 410MB |
| Native | main 210MB + WebView 140MB ≈ 350MB | ≤360MB |

结论：

```text
稳态：WebView 低约 40MB
峰值：Native 低约 50MB
CPU：Native 明显更低
进程：Native 少一个 WebView renderer
```

### 4.3 第二轮循环现象

两边都存在第二轮循环缓存行为：

- Native：main 从 180MB 到 210MB 后稳定；
- WebView：WebView 从 270MB 短时冲到 370MB 后回落。

判断：

```text
不是持续泄漏
是平台级 cache / pool / GC 行为
```

---

## 5. 关键问题与根因

### 5.1 双屏闪烁

根因：

```text
每块 surface 各自 AcquireSync(1) / ReleaseSync(0)
第二块屏拿不到 key
或者提前把 key 还给 worker
```

修复：

```text
renderer loop 每帧按 VideoPlayback Arc 指针去重
每帧只刷新一次本地纹理
所有 surface 只读同一 SRV
```

### 5.2 持续缓慢上涨

根因：

```text
D3D11_VIDEO_PROCESSOR_STREAM::pInputSurface 是 ManuallyDrop
每帧创建的 ID3D11VideoProcessorInputView 没有释放
```

修复：

```text
VideoProcessorBlt 后显式 ManuallyDrop::drop
```

相关修复：

```text
IMFDXGIDeviceManager::OpenDeviceHandle 未 CloseDeviceHandle
→ DxvaPipeline::drop 中关闭
```

### 5.3 切换视频内存暴涨

根因：

```text
pending FIFO 保存所有中间切换视频
每个视频都带独立 worker / D3D11 device / SourceReader / audio thread
```

修复：

```text
pending 只保留最新项
旧项立即 Drop
```

### 5.4 重复打开同一视频

根因：

```text
同一视频重复下发仍会 open_video
```

修复：

```text
VideoIdentity = path + len + mtime
current / transition / pending 中已有同身份视频则复用 Arc
```

### 5.5 图片/GIF 缓存

P0 将：

```text
FrameCache DEFAULT_MAX_BYTES: 64MiB → 16MiB
```

实机无明显变化，说明：

```text
图片缓存不是当前主进程基线主因
```

保留该改动，代价较低。

### 5.6 视频纹理格式

P1 将：

```text
BGRA8 shared/local texture → NV12 shared/local texture
```

理论上每路 2K 视频纹理：

```text
BGRA8 ≈ 28MB
NV12  ≈ 10MB
```

实机稳态总量没有明显拉开，说明：

```text
主进程内存大头不是纹理本身
更可能是 MF / DXVA / driver pool
```

保留该改动，降低 GPU/driver 资源更合理。

---

## 6. WebView 进程开销归属

WebView2 进程模型：

```text
1 个 browser process
1 个或多个 renderer process
GPU / audio / helper process
```

同一个 `CoreWebView2Environment` / user data folder 下：

```text
多个 WebView2 实例共享同一组 WebView2 进程资源
renderer 可服务多个 WebView2 实例
是否拆分受 site isolation / origin 影响
```

因此 WebView 方案中：

```text
主界面 WebView 已承担固定引擎初始化
透明壁纸 WebView 不是从零启动完整 Edge 引擎
```

固定成本包括：

```text
WebView2 runtime
Chromium browser process
GPU / audio helper
公共 DLL / runtime
profile / cache
```

增量成本主要是：

```text
renderer / frame / DOM / JS context
video element
合成表面
解码缓存
```

结论：

```text
不能简单用 WebView main 40MB 对比 Native main 210MB
应按总内存和峰值比较
```

---

## 7. Windows 平台 clear API 结论

微软没有提供适合本场景的运行时 “清空所有缓存” API。

可用的是生命周期 API：

| API | 作用 |
|---|---|
| `MFShutdown` | 关闭 MF 平台，一次 Startup 对一次 Shutdown，应用退出阶段调用 |
| `IMFMediaSource::Shutdown` / `IMFShutdown::Shutdown` | 释放具体对象资源 |
| SourceReader from URL | 释放 reader 时自动 shutdown media source |
| `CoUninitialize` | 退出线程 COM apartment |
| `ID3D11DeviceContext::ClearState` | 解绑上下文状态，不是清缓存 |
| `Flush` | 提交命令，不是清缓存 |
| `IDXGIDevice3::Trim` / working set trim | hint，不是确定性释放 |

结论：

```text
优化方向不应是找 clear API
而应是减少创建次数、收敛生命周期、隔离对象所有权
```

---

## 8. 子进程评估

将 Native renderer / decoder 拆到独立子进程：

### 收益

```text
main 进程账面下降
crash isolation
生命周期隔离
与 WebView 多进程模型更一致
```

### 成本

```text
IPC 通道
共享纹理句柄传递
句柄生命周期管理
进程崩溃恢复
额外进程基础开销
共享 DLL / driver pool 可能重复计入
```

### 共享纹理要求

当前 `GetSharedHandle()` 已不推荐用于跨进程。

应使用：

```text
D3D11_RESOURCE_MISC_SHARED_NTHANDLE
IDXGIResource1::CreateSharedHandle
DuplicateHandle
ID3D11Device1::OpenSharedResource1
```

### 评审结论

```text
不建议把子进程作为降低总内存的首选
它主要是隔离和记账，不是省内存
```

---

## 9. 全局 Worker 评审

### 9.1 目标

从：

```text
每个 VideoPlayback 独占 worker / D3D11 device / device manager
```

改为：

```text
全局唯一 VideoWorker
内部维护多个 VideoSession
共享一个 D3D11 device / context / device manager / video context
```

### 9.2 职责划分

| 层 | 职责 |
|---|---|
| Renderer | surface、swapchain、绘制、Present、本地纹理复制 |
| VideoWorker | MF、DXVA、SourceReader、VideoProcessor、帧时钟 |
| VideoSession | 单视频文件状态、shared texture、timing、pause/reset |
| WallpaperMedia | 轻量 session handle |

### 9.3 命令模型

建议命令：

```text
OpenSession
CloseSession
Pause
Resume
Reset
SetVolume
Shutdown
```

建议事件：

```text
FrameReady
SessionClosed
Error
```

### 9.4 生命周期

活跃集合来源：

```text
surfaces[*].video_arcs()
```

必须包含：

```text
current wallpaper
transition.to
pending
```

生命周期策略：

```text
active set 非空 → session 存在
active set 清零 → grace period 后关闭 session
所有 session 关闭 → 关闭 worker
新视频出现 → 重建 worker
```

建议 grace period：

```text
1–2 秒
```

避免快速切换时 worker 反复开关。

### 9.5 MF 生命周期

全局 Worker 后更适合：

```text
第一个 session → MFStartup
最后一个 session 关闭 → MFShutdown
```

比当前 renderer 全程持有 MF 更贴合实际使用。

### 9.6 收益

```text
减少重复 D3D11 device
减少重复 device manager
减少重复 VideoContext
减少重复 worker thread
减少快速切换时的 driver pool 反复创建
生命周期更集中
架构边界更清晰
```

### 9.7 限制

全局 Worker 不会消除：

```text
单个 60MB 视频的 MF parser / sample table
单个视频的 DXVA reference frame pool
当前视频纹理常驻
```

风险：

```text
independent 多视频串行解码
```

若未来多路 4K 高帧率视频成为常态，可演进为 worker pool，但接口应保持 session-based。

### 9.8 评审结论

```text
全局 Worker 不应以“显著降低单视频稳态内存”为目标
它的价值在于架构清晰、生命周期收敛、多视频场景稳定性
```

建议作为下一阶段架构项执行。

---

## 10. 决策清单

### 已定

| 项 | 决策 |
|---|---|
| 保留 DXVA worker + shared texture | 保留 |
| 保留 frame-level keyed mutex + local copy | 保留 |
| 保留 NV12 纹理路径 | 保留 |
| 保留 FrameCache 16MiB | 保留 |
| 保留视频身份复用 | 保留 |
| 继续寻找 clear cache API | 不做 |
| 子进程拆 decoder | 不作为内存优化首选 |
| 全局 Worker | 已实施并实机验收通过 |
| session 级 GPU 生命周期 | 已实施：单 worker thread + 每 session 独立 device/manager |
| `VIDEOPROCESSORBLT_OUTPUTHAZARD` | 已解决：Blt staging 隔离 + CopyResource 到 shared texture |
| 循环高倍速 | 已解决：MF_PD_DURATION + 首轮最大帧结束时间 |
| fade 期间视频/音频继续推进 | 已解决：pause reason 合并 + fade 中暂停全部视频源，结束后 reset/resume |
| 新视频音频提前播放 | 已解决：audio 线程以 paused 状态启动，只有 Play 命令才 Start WASAPI |

### 待办

| 项 | 状态 |
|---|---|
| 音频链路修复后复查内存 | 已完成：音频常驻约 10MB，用户确认可接受 |
| Worker 线程真正退出 / 按需重启 | 暂缓；session 关闭释放 GPU 已覆盖主要收益 |
| 音频链路加载失败修复 | 已完成：`bec0b9e`，有音轨视频实机有声 |
| 合并前 squash 测试分支 | 已完成：2026-11-22 在本分支 squash 试错提交并 force push，未单独切干净分支 |

---

## 11. 后续建议

当前阶段不建议继续追逐稳态总内存。

原因：

```text
Native 与 WebView 稳态差距约 40MB
这部分主要来自平台级 cache / runtime 归属
继续小修小补收益很低
```

建议路线：

1. 先接受当前 Native 方案的目标定位：
   ```text
   低 CPU
   低峰值
   少 WebView renderer
   ```

2. 全局 Worker + session 级 GPU 已完成当前阶段实施：
   ```text
   one renderer
   one idle-resident video worker
   N sessions
   one WorkerGpu per session
   session close releases its GPU/MF/DXVA resources
   ```

3. 若硬指标是 main 进程低于 50MB，再评估子进程方案：
   ```text
   明确其主要收益是隔离和记账
   不是总内存优化
   ```

4. 音频修复后重新评估 AudioPlayback 对内存的影响。

---

## 12. 验收标准与结果

全局 Worker 阶段验收标准：

1. independent / mirror / extend 全部稳定 —— 已通过；
2. 同一视频身份只对应一个 session —— 已通过；
3. 当前 / transition / pending 中的视频均计入活跃集合 —— 已通过；
4. 活跃集合清空后 session 能关闭 —— 已通过；
5. 所有 session 关闭后 GPU 资源释放 —— 已通过；
6. Worker 线程保留空闲，不立即退出 —— 当前设计；
7. `MFStartup` / `MFShutdown` 成对 —— 已保留在 renderer 生命周期；
8. 快速连续切换不产生 worker 残留 —— 已通过；
9. session 关闭时对应 D3D11 device / manager 整体释放 —— 已通过；
10. CPU 仍保持约 1% —— 已通过；
11. 峰值内存不高于当前方案 —— 已通过；
12. `0x80070057 / VIDEOPROCESSORBLT_OUTPUTHAZARD` 消失 —— 已通过；
13. 循环后高倍速消失 —— 已通过；
14. 多视频切换后内存正常回落，稳态约 +20MB —— 已通过。

---

## 13. 相关提交

> 当前分支历史已重写为 16 个语义化提交；下表只列当前可达 commit。更早的诊断 / revert / A/B hash 已不再存在，仅在技术文档“方案演进”段落中作为历史叙述保留。

| Commit | 说明 |
|---|---|
| `990e18c` | 添加 Native renderer：D3D11 + DirectComposition + renderer 线程 + frame cache |
| `273046e` | 添加跨屏 mirror / extend 渲染 |
| `c0cc441` | 缓存解码帧并优化 dev build profile |
| `f53903d` | 使用 Media Foundation 实现 Native 视频解码 |
| `61d9c48` | 添加 Native↔Native crossfade |
| `4fdba00` | 恢复 WebView capability fallback |
| `8fc0a1f` | display mode 变化时同步 Native 壁纸 |
| `1f7b2db` | 实现 DXVA 硬解、NV12 shared texture、dedicated worker 与多屏帧同步 |
| `85dc97c` | FrameCache 预算降到 16MiB |
| `af00651` | 使用单一 video worker 管理所有 session |
| `bec0b9e` | 稳定 WASAPI 音频格式协商与播放 |
| `594ce4e` | 修复首帧前绿屏 / 黑屏 |
| `7027365` | 修复 `OUTPUTHAZARD` 与循环 pacing，保留 D3D11 诊断 |
| `8db0ce0` | 每个 VideoSession 独立 WorkerGpu，关闭时释放 device/manager |
| `6f831d5` | fade / pending 期间暂停视频源与音频，结束后 reset/resume |
| HEAD（docs commit） | 汇总 Native renderer 技术文档与架构评审 |

---

## 14. 最终判断

Native 方案已经解决正确性和稳定性问题，但当前不是“最低稳态内存方案”。

它的真实定位是：

```text
低 CPU
低峰值
更少 WebView renderer
更可控的视频帧同步
```

WebView 方案的真实定位是：

```text
更低稳态总内存
但峰值更高
renderer 进程更多
CPU/GPU 成本更高
```

后续优化不建议继续围绕 clear API 或小范围缓存调参展开。当前已完成：

```text
架构清晰化 → 单 Worker thread + session 级 GPU 已实施
GPU 生命周期 → session 关闭即释放对应 D3D11 device/manager
正确性 → OUTPUTHAZARD / 循环高倍速 / 切屏冻结均已消除或确认不回归
稳态内存 → 纯图片 Native 有优势；视频 Native 优势在 CPU 与帧同步，不以最低内存为目标
main 进程账面降低 → 子进程隔离仍不推荐
```

音频链路加载失败已修复并实机验收通过；有音轨视频正常播放，音频常驻约 10MB，用户确认可接受。当前无待处理重点，进入交付收尾。
