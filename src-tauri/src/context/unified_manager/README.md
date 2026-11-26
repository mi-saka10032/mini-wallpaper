# unified_manager：统一壁纸管理器

## 定位

`UnifiedWallpaperManager` 是壁纸状态的**唯一聚合入口**：所有显示器 slot、Native 渲染后端、WebView 窗口、配置快照都收在它的一把锁（`AppContext::unified_manager: Mutex<...>`）下。

它取代了早期的 `RenderCoordinator` / `WallpaperWindowManager`，把两者拉平成一个对象。TaskRegistry、各 command、前端事件都经它串行操作壁纸状态。

## 目录结构

| 文件 | 职责 |
|---|---|
| `mod.rs` | 类型定义（`MonitorSlot` / `UnifiedWallpaperManager`）、构造与 init、`build_snapshot` 等共享辅助、extend viewport 推送 |
| `monitors.rs` | 显示器同步：系统枚举 → DB 写入 → slot 更新 → reconcile |
| `reconcile.rs` | reconcile 编排：读 DB → diff → 逐项派发 → 局部推进 observed |
| `engine.rs` | **纯算法引擎**：配置快照类型、diff 计算、派发计划（无 IO，见下） |
| `native.rs` | Native 渲染链路的 `impl`：target 生命周期、壁纸更新、音量/模式、GPU 资源 |
| `webview.rs` | WebView 窗口链路的 `impl`：窗口创建/销毁、事件通知、几何换算 |
| `errors.rs` | 设备丢失回调与统一错误出口 |

`impl UnifiedWallpaperManager` 分散在 mod / monitors / reconcile / native / webview 五个文件——Rust 允许同一类型的 impl 分文件写，这是按功能块组织大型类型的标准手法。

## 核心机制：action → reconcile → dispatch

这是整个模块的设计核心，取代了旧的"命令直达 + 轮询兜底"模式。

### 三个状态

| 状态 | 位置 | 含义 |
|---|---|---|
| **DB desired** | `monitor_configs` + app_settings 表 | 用户/前端写入的目标状态 |
| **observed** | `self.observed: ConfigSnapshot` | 已成功应用到渲染层的状态 |
| **slot** | `self.slots: HashMap<String, MonitorSlot>` | 每屏的运行时资源（窗口 label / rect / DPI / backend） |

reconcile 比较 observed 与 desired，只派发差异项；派发成功才推进 observed。

### 数据流

```
写入方(command / scheduler / carousel)
    │  action 事务：写 DB + 推导 ReconcileScope
    ▼
manager.reconcile(scope)
    ├── build_snapshot(DB) → desired
    ├── engine::diff_snapshots(observed, desired, scope) → Vec<ConfigDiff>
    ├── engine::plan_dispatch(diffs, desired) → Vec<DispatchAction>
    ├── 逐项 dispatch_one(action) → native / webview 链路执行
    └── engine::patch_observed(observed, diffs, applied) → 只推进成功项
```

**失败重试**：派发失败的项不推进 observed，下一次 reconcile 会重新 diff 出来重试。这是取消常驻轮询后**唯一的重试来源**，所以 `patch_observed` 必须按差异项局部推进（不能整段覆盖）。

### ReconcileScope：范围裁剪

一次写操作只应影响它实际改动的范围：

- `All` —— global 级 action（display_mode / volume）或屏幕集合变化
- `One(monitor_id)` —— 单屏字段变化
- `Many(Vec<String>)` —— 多屏批量变化（如屏幕几何成组变化）

`engine::diff_snapshots` 按 scope 过滤，避免单屏改动触发全组重整。外部（runtime / commands）只通过 manager 门面 re-export 的 `ReconcileScope` 指定范围。

## engine.rs：纯算法引擎

614 行、无 IO、可独立单测。它回答三个问题：

| 函数 | 输入 → 输出 |
|---|---|
| `diff_snapshots` | `(observed, desired, scope)` → `Vec<ConfigDiff>` |
| `plan_dispatch` | `(diffs, desired)` → `Vec<DispatchAction>` |
| `patch_observed` | `(observed, desired, diffs)` → 就地推进成功项 |

关键类型：

- `ConfigSnapshot` = `HashMap<monitor_id, MonitorSnapshot>` + `GlobalSnapshot`（display_mode / volume）
- `MonitorSnapshot` 只保留 3 个影响窗口的字段：`wallpaper_id` / `fit_mode` / `active`。`id` / `updated_at` 等代理字段变化不代表渲染状态变化，`collection_id` / `play_mode` 等只服务轮播定时器，都不进快照
- `ConfigDiff`：MonitorAdded / MonitorRemoved / MonitorChanged / DisplayModeChanged / VolumeChanged
- `DispatchAction`：EnsureTarget / DestroyTarget / UpdateWallpaper / ClearWallpaper / SetFitMode / SetDisplayMode / SetVolume

**注意**：`DisplayModeChanged` 只描述模式变化本身，整段编排（如切到 extend 时逐屏重算 viewport）由派发层执行——engine 不重复产出 fit_mode 等字段动作，否则同一动作派发两次。

## 显示器同步（monitors.rs）

系统热插拔与初始化走 `sync_monitors_from_system`：

```
系统枚举 (enumerate_monitors)
    → 写 DB：新增屏 insert / 断开屏 active=false
    → 改 slot：新增/移除
    → reconcile(All)
```

**顺序是硬约束**：先写 DB → 再改 slot → 最后 reconcile。reconcile 从 DB 读 desired，若 slot 先于 DB 变更会得到错误的 diff 基线。

## Native 与 WebView 两条链路

`dispatch_one` 按目标 backend 调不同 `impl` 文件的方法：

| 动作 | Native (`native.rs`) | WebView (`webview.rs`) |
|---|---|---|
| EnsureTarget | `ensure_target` 创建 RenderSurface | `create_and_show` 建窗并嵌入 |
| UpdateWallpaper | Native 渲染器下发壁纸 | `update_window` emit 事件 |
| SetFitMode | `set_fit_mode` | `notify_fit_mode_changed` |
| DestroyTarget | `destroy_target` | `destroy_window` |

两条链路的**模式切换**由 `apply_backend`（native.rs）统一裁决：切到 mirror/extend 时按 `resolve_targets` 决定哪些屏走 Native（共享一份渲染）还是 WebView。

## 对外契约（跨模块只写输入输出）

| 消费者 | 输入 / 输出 | 契约 |
|---|---|---|
| TaskRegistry / commands | `reconcile(ReconcileScope)` | 经 manager 门面的 `ReconcileScope`；调用方不依赖 engine 内部 |
| TaskRegistry / commands | 读 `slots()` / `observed()` | 只读引用，不得直接改；修改必须走 action → reconcile |
| carousel | `ReconcileScope::One(mid)` | 轮播换图写 DB 后触发单屏 reconcile |
| desktop_renderer | `update_wallpaper` / `set_volume` / `set_paused` 等 | 由 native.rs 的 impl 转给渲染线程；渲染线程不反向依赖 manager |
| AppContext | `init()` | setup 之后异步调用；桌面嵌入依赖消息循环，不能在 setup 的 block_on 里同步做 |

## 设计约束

- **串行**：manager 由 `AppContext` 的 Mutex 保证所有 reconcile 不交错
- **无反向依赖**：native.rs 不持有 AppHandle 之外的上层类型；设备丢失回调由上层注入
- **并发安全**：所有跨 `'static` 边界持 `AppHandle`，经 `app.state::<Arc<AppContext>>()` 访问 manager
