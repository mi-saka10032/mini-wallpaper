# 架构重整：action 驱动的批量 diff 收敛

> 状态：**方案定案，待实施**（2026-11-23 15:19 用户最终收敛）
>
> 演进：轮询快照（10:54 提案）→ 事务成功后直调 reconcile（14:22）→ **action 归一：一个 action = 一次事务 = 一次 reconcile（15:19）**。
> 15:19 明确：**display_mode 同样走 diff，只是批量**；无常驻 task、无旁路派发、无 `fast_forward_observed`。

---

## 0. 决策摘要

| # | 决策点 | 结论 |
|---|---|---|
| 1 | 触发机制 | **action 驱动**。`写入点事务提交成功 → 触发一次 Manager::reconcile()`。无常驻 task、无轮询、无 channel |
| 2 | 事务 | 所有 DB 写（含批量 monitors）在**同一事务**内完成；**只有 commit 成功才触发副作用** |
| 3 | action 粒度 | 单 monitor action（普通 upsert）→ 单显示器 diff；**批量 action（display_mode）→ 一次 diff 覆盖 N 个显示器**。display_mode 只有一个 action，因此只触发一次副作用 |
| 4 | 副作用次数 | 一次 action 只调一次 `reconcile()`；其内部按 diff 结果派发多条动作，但**不再写 DB、不再二次触发** |
| 5 | 数据语义 | Manager 查 `monitor_config` + `app_setting` 白名单，与 `observed` diff，只更新差异项 |
| 5a | **scope 分级** | monitor 级 action（`wallpaper_id` / `fit_mode`）→ 只查/派发单个 `monitor_id`；global 级 action（`display_mode` / `volume`）→ 覆盖全部 `monitor_id`（多屏更新壁纸、多屏复用同一张壁纸、多屏共用音频调整 volume 等场景）。**scope 由事务实际写入的 monitor_id 集合推导**，不由 action 硬编码 |
| 5b | observed 局部推进 | scoped reconcile 只 patch `observed` 中该 scope 的条目，不可整体覆盖，否则其它屏的 observed 丢失 |
| 6 | init | Manager 随 ctx 初始化，含两个方法（MonitorAPI 取显示器参数、读 DB）；init 都调，后续调度只读 DB。无 `wallpaper_id` 不派发 renderer / webview |
| 7 | 前端 | **取消前端热插拔**；显示器参数只读，前端仅保留 `wallpaper_id` / `collection_id` 等可编辑业务字段 |
| 8 | 新增任务 | **M20 / P2**：Rust 热插拔事件监听 + 前端显示器参数彻底只读（只消费 Rust emit） |
| 9 | 过渡 | 直接切，不双跑、不设 feature flag |
| 10 | 范围 | renderer 线程内部改造延后到下一个重构 part；本次只做「写入点事务收口 + 整合调度器」 |

### 撤销的机制（15:19 定案后不再需要）

| 曾提出 | 撤销原因 |
|---|---|
| `config_watcher` / 16ms 轮询 / `Notify` | 改为 action 驱动 |
| `fast_forward_observed()` / 事务内冻结快照 / 并发写入被吞 | 前提是「display_mode 绕过 diff 自派发」。定案后它走**同一次 reconcile**，observed 由该次 reconcile 正常推进，无需快进 |
| C4 收敛循环 | 所有 DB 写前移到事务，派发阶段零写，不存在"派发内部产生新变更" |
| 6×N 条重复派发风险 | 同上：observed 与 DB 在同一次 reconcile 内对齐 |

---

## 1. 现状问题

monitor_config 有 **5 个写入源**、**3 条副作用路径**，散布 **11 个文件**：

| 写入源 | 副作用方式 |
|---|---|
| 前端 `upsert` × 3 | 写 DB → 手动 `syncWallpaperWindows` |
| `doSyncMonitors` | 批量 upsert → `syncWallpaperWindows`（`monitorSync.ts:124`） |
| `upsert_monitor_config` command | 写 DB → 管 timer → fitMode → update_wallpaper（4 条内联） |
| 轮播 tick `carousel.rs` | 直接写 DB + `coordinator.update_wallpaper`，绕过 command |
| 删除联动 / 暂停 | 直接写 DB + `RenderNotification` 批量派发 |

`display_mode` 的现状尤其分散：`setting_effects.rs:170` 的 `sync_config_from` **逐条 await 写 N 个屏**，再逐屏 `set_fit_mode`、管 timer、广播——既无事务保护，也是"一次 action 触发一串副作用"的典型。

---

## 2. 目标架构

### 2.1 核心：一个 action = 一次事务 = 一次 reconcile

```
action (单 monitor upsert / 批量 display_mode / volume / 轮播 tick / 删除联动)
   │
   ├─ BEGIN
   │    单条写 或 批量写 N 条 monitor_config + app_setting
   │  COMMIT  ── 失败则不触发任何副作用
   │
   └─ commit 成功 → Manager::reconcile(scope)   ← 且仅此一次
          │  tokio::Mutex 整段持锁  [C3]
          ├─ desired = read_snapshot(db, scope)  // 只查 scope 内的 monitor + app_setting 白名单
          ├─ diff(observed, desired)             // 一次覆盖本次事务产生的全部差异
          ├─ 逐项派发（组级动作共用同一个 now）
          └─ patch observed 中该 scope 的条目     [C2]
```

**scope 由事务实际写入的 monitor_id 集合推导**：写 1 条 → `Scope::One(mid)`；写 N 条或改 `app_setting` 全局键 → `Scope::All`。这样事实源落在事务上，而不是让 action 自己声明范围。

**mirror/extend 的边界**：`wallpaper_id` 看似单屏改动，但 `resolve_targets(monitor_id, is_sync=true)`（`render_coordinator.rs:498-509`）返回的是 `rects` ∪ `backends` 的全部屏。因此同步模式下事务必须一并写 N 条 config，scope 自然变成组级——不能把 `wallpaper_id` 一律硬编码成 `Scope::One`。

### 2.2 两类 action

| action | 事务内容 | scope | diff 产出 / 派发 |
|---|---|---|---|
| 单 monitor upsert | 1 条 config | `One(mid)` | 该屏字段差异 → 单屏 set / update / timer |
| **display_mode（批量）** | `app_setting.display_mode` + **N 条 config**（原 `sync_config_from` 的 6 字段 × N 屏） | `All` | `DisplayModeChanged` + N 屏字段差异 → **组级批量** `update_monitors`，共用同一帧时钟 |
| volume | `app_setting.global_volume` | `All` | `VolumeChanged` → set_volume + 全 WebView 广播 |
| 轮播 tick / 删除联动 | 单条或批量 config | 按写入条数 | 对应字段差异 |
| 同步模式下换壁纸 | N 条 config（mirror / extend 组） | `All` | 整组共用同一帧时钟下发 |

新增 service 方法 `update_monitors_batch(...)`：在**同一事务**内写 N 条，替换 `sync_config_from` 逐条 `active_model.update(db)` 的旧实现（`sync_config_from` 全项目仅 `setting_effects.rs:170` 一处调用）。

### 2.3 字段 → 动作映射

**快照输入**：`monitor_configs` 全表 + `app_setting` 白名单（`display_mode`、`global_volume`）。
其余 key 不进快照：`theme` / `language` / `close_to_tray` / `accent_color` / 回收站两项无副作用；`pause_on_fullscreen`、5 个 `shortcut_*` 归 Scheduler 不动。

| 变更字段 | 动作 |
|---|---|
| 新增 `active=true` 且有 `wallpaper_id` | Win32 rect + DPI → 创建 surface/webview → 首帧 |
| 新增 `active=true` 无壁纸 | 只登记几何，不建渲染目标 |
| `active` true→false | 销毁该屏 surface + webview |
| `wallpaper_id` 变更 / 置空 | resolve_backend → 渲染更新 / clear_wallpaper |
| `fit_mode` 变更 | set_fit_mode |
| `collection_id` / `play_mode` / `play_interval` / `is_enabled` | 管理轮播 timer |
| **`DisplayModeChanged`** | 按 independent / mirror / extend 分支执行**整段编排**（组级重下发、timer 管理、WebView 广播）；diff 只负责发现变化 |
| `VolumeChanged` | set_volume + `VolumeChangedPayload` 广播 |

**实现注意（动作去重）**：`DisplayModeChanged` 的编排里原有"逐屏 `set_fit_mode`"（`setting_effects.rs:126-130`），而同一次 diff 里 N 屏的 `fit_mode` 字段差异也会产出 `set_fit_mode`。二者必须去重，否则重复派发。原则：**字段级差异交给字段动作，编排只做无法从字段推导的部分**（组级帧时钟、display_mode 广播、timer 组策略）。

### 2.4 UnifiedWallpaperManager

```rust
struct MonitorSlot {
    rect: MonitorRect,         // Win32 GetMonitorInfoW，物理像素
    dpi: u32,                  // GetDpiForMonitor（M18 用）
    backend: RenderBackend,
    webview_label: Option<String>,
    current_wallpaper_id: Option<i32>,
    current_fit_mode: String,
}

struct UnifiedWallpaperManager {
    db: DatabaseConnection,
    app_handle: AppHandle,
    renderer: DesktopRenderer,     // 本次不改其内部
    slots: HashMap<String, MonitorSlot>,
    display_mode: DisplayMode,     // 与 slots 同级
    volume: u32,
    observed: ConfigSnapshot,     // 由每次 reconcile 成功后推进
}
```

`Arc<tokio::sync::Mutex<…>>` 包裹，写入点经 `app.state::<AppContext>()` 取用（合并现有 `render_coordinator()` / `window_manager()` 为 `unified_manager()`）。

### 2.5 init（一次性，随 ctx 初始化）

```
Manager::init()
  1. MonitorAPI 方法：EnumDisplayMonitors + GetMonitorInfoW + GetDpiForMonitor
     → Vec<MonitorInfo { device_name, rect, dpi }>          [B1/B2]
  2. 读 DB 方法：monitor_configs 全表 + app_setting 白名单
  3. reconcile 物理屏 ∩ DB（单事务）
       物理有 / DB 无 → 插入 config(active=true) 并登记 slot
       DB active / 物理无 → 置 active=false
       均有 → 更新 slot 的 rect / dpi
  4. 仅 wallpaper_id 非空的 slot → 创建 surface/webview + 首帧
  5. 生成初始 observed
```

后续调度**只读 DB**，不再调 MonitorAPI。

---

## 3. 执行分期（直接切，每期独立 commit）

### Phase 0：Win32 显示器枚举 + DPI（前置）

- `monitor_geometry.rs` 新增 `enumerate_monitors() -> Vec<MonitorInfo { device_name, rect, dpi }>`
- `EnumDisplayMonitors` + `GetMonitorInfoW` + `GetDpiForMonitor(MDT_EFFECTIVE_DPI)`
- 纯新增；Windows-only，需实机核对 rect / DPI 数值

### Phase 1：快照 / diff 引擎（纯跨平台逻辑 + 单测）

- `ConfigSnapshot`、`ConfigDiff` enum、`diff_snapshots(observed, desired)` 纯函数 + 逐字段单测
- **批量差异单测**：一次 desired 含 N 屏变化，断言 diff 一次产出全部、且字段动作与 `DisplayModeChanged` 编排去重后无重复
- **observed 推进语义**：派发失败不推进，下一次 reconcile 自动重放该项
- 此期不动现有代码，零实机依赖

### Phase 2：UnifiedWallpaperManager + init 接管

- 合并 `RenderCoordinator` / `WallpaperWindowManager` 为 Manager + slot map，包 `tokio::Mutex`
- 落地 2.5 init；ctx 初始化阶段 await 完成
- 建窗按目标屏 DPI 做物理→逻辑换算（**M18 / B3**）
- 直接切：`syncWallpaperWindows`、`create_wallpaper_window` command 的窗口编排停用

### Phase 3：action 事务收口

- 新增 `update_monitors_batch(...)`：单事务写 N 条，替换 `sync_config_from` 唯一调用点
- `set_setting` command 改为事务写（`app_setting_service::set` 接入 txn）+ commit 后一次 `reconcile()`
- 5 个写入源全部改为「事务写 + 一次 reconcile」；删除 command 的 4 条内联副作用、`RenderNotification` 路径
- `setting_effects.rs` 按 key 白名单部分摘除：仅 `DISPLAY_MODE` / `GLOBAL_VOLUME` 改走 action；`PAUSE_ON_FULLSCREEN`、5 个 `SHORTCUT_*` 归 Scheduler 不动
- **验收矩阵**：写入源 × 副作用逐格确认，确保每个 action 恰好一次 reconcile

### Phase 4：前端只读化

- `monitorConfigStore.init()` 退化为只读展示；删除 `useMonitorHotPlug.ts` 的 setInterval
- `upsert` / `upsertAll` / `syncConfigToAll` 只保留业务字段
- `monitorSync.ts` 的 `doSyncMonitors` / `syncWallpaperWindows` 整段删除
- 删除 `create_wallpaper_window` / `destroy_wallpaper_window` 对外 commands
- `useExtendViewport.ts` 改为消费 Rust 推送的物理 viewport + DPI

> **已知行为变化**：删除前端热插拔后，显示器插拔在 **M20** 落地前完全不响应（init 只在启动执行一次）。这是明确的临时回归。

---

## 4. 约束与风险

### 4.1 约束

| ID | 约束 | 理由 |
|---|---|---|
| **C1** | 事务 commit 成功后才调 `reconcile()`；所有相关写在**同一事务** | 事务内/回滚后的写入不得触发副作用；单连接池下也要等连接归还再读全量 |
| **C2** | `observed` 只记录**已成功应用**的状态；scoped reconcile **只 patch 该 scope 的条目**，不整体覆盖 | 派发失败不推进 → 下一次 reconcile 自动重放（唯一的重试来源）；整体覆盖会抹掉其它屏已确认的状态 |
| **C3** | 整段 `reconcile()` 用 `Arc<tokio::sync::Mutex<Manager>>` 持锁 | 并发 action 会基于同一旧快照重复/乱序派发；不可 `std::sync::Mutex` 跨 await |
| **C4** | **派发阶段零 DB 写**：所有写前移到事务 | 保证一次 action 只触发一次副作用，无需收敛循环或快进 |
| **C5** | init 随 ctx 同步完成；无条件登记 slot，仅 `wallpaper_id` 非空建窗 | 避免首个 action 读到半成品快照；覆盖首次启动 DB 空 |

### 4.2 Blockers

| ID | 状态 |
|---|---|
| **B1** `EnumDisplayMonitors` / **B2** `GetDpiForMonitor` | 用户确认提前做 → Phase 0 |
| **B3** WebView 建窗只收逻辑像素 | Phase 2 用 B2 的 DPI 换算（即 M18） |
| **B4** `monitor_id` 同源 | 已确认（`monitor_geometry.rs:12-14`） |
| **B5** 前端热插拔冲突 | 已确认取消，另立 M20 |
| **B6** app_setting 收编 | 已确认：事务 + 按 key 白名单部分摘除 + display_mode 为分支编排 |

### 4.3 风险

| ID | 风险 | 缓解 |
|---|---|---|
| **R1** | 某个 action 漏挂 `reconcile()` → 副作用静默丢失（无轮询兜底） | Phase 3 写入源 × 副作用验收矩阵；用户已明确不考虑此代价 |
| **R2** | 字段动作与 `DisplayModeChanged` 编排重复派发 | 2.3 去重原则 + Phase 1 单测 |
| **R3** | 直接切无回退 | 分期 commit + 每期 Windows 实机验收 |

### 4.4 范围排除

- renderer 线程内部改造（mpsc → 轮询、依赖注入、"渲染控制面/渲染面解耦"）——下一个重构 part
- Rust 热插拔事件监听 / 前端参数只读化 → **M20**
- `selection` 运行时切换 —— 用户已确认不做

---

## 5. 对既有 M 任务的影响

| M 编号 | 影响 |
|---|---|
| **M14** | 拆分：Win32 枚举 + init 登记 + DPI 换算由 Phase 0/2 吸收；事件监听与前端只读化独立为 **M20** |
| **M20 / P2（新增）** | Rust 热插拔事件监听（`WM_DISPLAYCHANGE` / `WM_DPICHANGED`）+ 前端显示器参数彻底只读 |
| **M18** | 被吸收：Phase 2 建窗 DPI 换算 |
| **M19** | Phase 2 建立 Manager 后 renderer → manager 通道可用，可追加 |
| **M16** | Phase 1 diff 引擎自带单测 |
| **M17** | 无直接影响 |
| renderer 解耦重构（无编号） | 排在本次之后，本次不碰 renderer 内部 |

---

## 6. 实施顺序

```
Phase 0 ✅ 已落地（Win32 枚举 + DPI，待 Windows 实机核对 rect/dpi）
  ↓
Phase 1 ✅ 已落地（diff 引擎 + 8 组单测，纯逻辑零实机依赖）
  ↓
Phase 2 → commit + Windows 实机（合并 Manager + init 接管，改动最大）
  ↓
Phase 2 → commit + Windows 实机（Manager + init + 窗口接管 + M18）
  ↓
Phase 3 → commit + Windows 实机（action 事务收口 + 验收矩阵）
  ↓
Phase 4 → commit + 前端只读化 + 最终验收
```

---

## 7. 已确认（15:28）

1. **统一化**：所有 action（含 volume、普通 upsert、轮播 tick、删除联动）全部走「事务写 + 一次 `reconcile()`」，彻底取消旁路直接注入。✅
2. **scope 分级**：monitor 级 → 单 `monitor_id`；global 级 → 全部 `monitor_id`。✅ 实现上由事务写入的 monitor_id 集合推导，并覆盖 mirror/extend 的组级场景（见 2.1）。

### Phase 0 交付（已完成，未 commit）

| 文件 | 改动 |
|---|---|
| `Cargo.toml` | `windows-sys` 加 `Win32_UI_HiDpi` feature |
| `monitor_geometry.rs` | 新增跨平台 `MonitorInfo { rect, dpi }` + `scale_factor()`；Windows 实现 `enumerate_monitors()`（`EnumDisplayMonitors` + `GetMonitorInfoW` + `GetDpiForMonitor(MDT_EFFECTIVE_DPI)`）；非 Windows stub 返回空集 |

验证：`cargo fmt --check` / `cargo check` / `cargo test --lib`（51 passed）全绿；Windows-only 代码用 `/tmp/monitor-geom-probe` 镜像整个模块跑 `cargo check --target x86_64-pc-windows-msvc`，**0 error / 0 warning**。

### Phase 1 交付（已完成，未 commit）

| 文件 | 改动 |
|---|---|
| `ctx/reconcile.rs`（新） | `MonitorSnapshot` / `GlobalSnapshot` / `ConfigSnapshot`；`ReconcileScope::{One, Many, All}` + `from_written_monitors()`；`ConfigDiff`（Added / Removed / Changed / DisplayModeChanged / VolumeChanged）；`diff_snapshots()`；`patch_observed()` |
| `ctx/mod.rs` | 注册 `pub mod reconcile` |

关键语义：
- `MonitorChanged` 把同屏多个字段变化合并成一项；`DisplayModeChanged` 独立成项，**不重复产出 set_fit_mode 等字段动作**（2.3 去重原则）。
- `patch_observed()` 只推进 `applied` 里的差异，绝不整体覆盖 observed——失败项下次 diff 会重放（C2 重试）、其它屏状态不受影响。
- scope 由事务写入的 monitor_id 集合推导，`All` 才比较 global 字段。

验证：`cargo fmt --check` / `cargo check`（0 warning）/ `cargo test --lib` **59 passed**（新增 8 组：七字段全覆盖、scope 单屏隔离、批一次产出 N 屏、global 与字段动作去重、增删屏、失败重放、局部 patch 不动其它屏、scope 推导）。

待 Windows 实机核对 Phase 0 的枚举数量 / `device_name` / `dpi` 后进入 **Phase 2**。
