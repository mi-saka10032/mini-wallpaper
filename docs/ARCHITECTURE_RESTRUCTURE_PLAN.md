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
| 单 monitor upsert | 1 条 config | `One(mid)` | 该屏窗口字段差异 → 单屏 set / update（timer 仍由 Scheduler 处理） |
| **display_mode（批量）** | `app_setting.display_mode` + **N 条 config**（原 `sync_config_from` 的 6 字段 × N 屏） | `All` | `DisplayModeChanged` + N 屏字段差异 → **组级批量** `update_monitors`，共用同一帧时钟 |
| volume | `app_setting.global_volume` | `All` | `VolumeChanged` → set_volume + 全 WebView 广播 |
| 轮播 tick / 删除联动 | 单条或批量 config | 按写入条数 | 对应字段差异 |
| 同步模式下换壁纸 | N 条 config（mirror / extend 组） | `All` | 整组共用同一帧时钟下发 |

新增 service 方法 `update_monitors_batch(...)`：在**同一事务**内写 N 条，替换 `sync_config_from` 逐条 `active_model.update(db)` 的旧实现（`sync_config_from` 全项目仅 `setting_effects.rs:170` 一处调用）。

### 2.3 字段 → 动作映射

**快照输入**：`monitor_configs` 的**窗口相关 3 字段**（`wallpaper_id` / `fit_mode` / `active`）+ `app_setting` 白名单（`display_mode`、`global_volume`）。

排除项：
- `id` / `updated_at`：代理主键与时间戳，纳入会让任何写入都产出伪差异。
- `collection_id` / `play_mode` / `play_interval` / `is_enabled`：**只服务轮播定时器，归 Scheduler**（`manage_carousel_timer` / `should_start_timer`），且 `carousel.rs:99` 每次 tick 动态重读 config，不产生窗口动作。轮播换图最终写 `wallpaper_id`，仍会被 3 字段 diff 捕获。本次重整范围是「窗口调整相关副作用」，timer 不在其中。
- `app_setting` 其余 key：`theme` / `language` / `close_to_tray` / `accent_color` / 回收站两项无副作用；`pause_on_fullscreen`、5 个 `shortcut_*` 归 Scheduler 不动。

| 变更字段 | 动作 |
|---|---|
| 新增 `active=true` 且有 `wallpaper_id` | Win32 rect + DPI → 创建 surface/webview → 首帧 |
| 新增 `active=true` 无壁纸 | 只登记几何，不建渲染目标 |
| `active` true→false | 销毁该屏 surface + webview |
| `wallpaper_id` 变更 / 置空 | resolve_backend → 渲染更新 / clear_wallpaper |
| `fit_mode` 变更 | set_fit_mode |
| ~~`collection_id` / `play_mode` / `play_interval` / `is_enabled`~~ | **不进快照**：timer 归 Scheduler，本次不收编 |
| **`DisplayModeChanged`** | 按 independent / mirror / extend 分支执行**整段编排**（组级重下发、timer 组策略委托 Scheduler、WebView 广播）；diff 只负责发现变化 |
| `VolumeChanged` | set_volume + `VolumeChangedPayload` 广播 |

**实现注意（动作去重）**：`DisplayModeChanged` 的编排里原有"逐屏 `set_fit_mode`"（`setting_effects.rs:126-130`），而同一次 diff 里 N 屏的 `fit_mode` 字段差异也会产出 `set_fit_mode`。二者必须去重，否则重复派发。原则：**字段级差异交给字段动作，编排只做无法从字段推导的部分**（组级帧时钟、display_mode 广播、timer 组策略）。

### 2.4 UnifiedWallpaperManager

**Phase 2 实际落地形态：组合复用**，不物理搬动 `RenderCoordinator`（782 行）与 `WallpaperWindowManager`（431 行）的 Windows-only 渲染代码。Manager 作为统一入口持有二者的 `Arc<Mutex<…>>`，init 复用 `ensure_target` 建目标；两者边界待 Phase 3 收口调用点后再合并。

```rust
struct MonitorSlot {
    rect: RenderRect,   // 物理像素虚拟桌面坐标 x/y/width/height
    dpi: u32,           // 目标屏 GetDpiForMonitor（M18 换算用）
    has_target: bool,   // 是否已有渲染目标
}

struct UnifiedWallpaperManager {
    db: DatabaseConnection,
    coordinator: Arc<Mutex<RenderCoordinator>>,
    window_manager: Arc<Mutex<WallpaperWindowManager>>,  // Phase 3 统一派发 WebView 动作
    slots: HashMap<String, MonitorSlot>,
    observed: ConfigSnapshot,   // init 生成初始状态，后续 reconcile 成功后推进
}
```

`Arc<tokio::sync::Mutex<…>>` 包裹，注册为 `AppContext::unified_manager`。init 失败只记 error 不阻断启动，前端原有 `syncWallpaperWindows` 路径仍能兜底。

### 2.5 init（一次性，随 ctx 初始化）

```
Manager::init()
  1. MonitorAPI 方法：EnumDisplayMonitors + GetMonitorInfoW + GetDpiForMonitor
     → Vec<MonitorInfo { device_name, rect, dpi }>          [B1/B2]
  2. 读 DB 方法：monitor_configs 全表 + app_setting 白名单 → 生成初始 observed
  3. 物理屏 ∩ DB：DB 有该屏且 active → 登记 slot（rect/dpi）
       register_monitor 先于「有没有壁纸」判断，保证无壁纸的屏也进 extend union
  4. 仅 wallpaper_id 非空 → coordinator.ensure_target() 建 surface/webview + 首帧
```

**Phase 2 刻意只读 DB、不写 DB**：新屏登记（插入 config / 失活消失的屏）属于写入源收口，与 Phase 3 停用前端 `doSyncMonitors` 一起做，避免两边都写。首次启动 DB 为空时由前端原路径完成登记与建窗，Manager 下一次启动接管。

前端不会重复建窗：`syncWallpaperWindows` 先调 `getActiveWallpaperWindows()`（`monitorSync.ts:30`）恢复已有窗口集合，并在 `:51` 跳过已存在的 monitor；Manager 经 `ensure_target` 建的目标会进入 `coordinator.active_ids()`。

非 Windows 平台 `enumerate_monitors()` 返回空集，init 退化为只构建快照，现有前端链路照常工作。

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

### Phase 3：action 事务收口（已落地，部分收尾待 Phase 5）

- ✅ service 泛型化：`upsert` / `get_all` / `get_by_monitor_id` / `update_wallpaper_id` / `clear_wallpaper_id` / `sync_config_from` / `wallpaper_service::get_active_by_id` 由 `&DatabaseConnection` 改为 `&C: ConnectionTrait`，支持在事务内复用
- ✅ 新增 `update_monitors_batch(...)`：单事务写 N 条，替换 `sync_config_from` 在 setting_effects 的逐条调用点
- ✅ `ctx::reconcile` 增加 `DispatchAction` + `plan_dispatch()`：diff → 动作映射可脱离 Windows 单测（fit_mode 先于壁纸；新增屏只发一次 EnsureTarget）
- ✅ `UnifiedWallpaperManager::reconcile(scope)`：读 DB 快照 → diff → 逐动作派发 → `patch_observed` 局部推进；display_mode 的 `set_display_mode` 在所有 fit_mode 之前执行
- ✅ 写入源收口：
  - `upsert_monitor_config` command：4 条内联渲染副作用 → 一次 `reconcile(One(mid))`（timer 仍归 Scheduler）
  - `setting_effects` `GLOBAL_VOLUME` / `DISPLAY_MODE`：渲染副作用 → `reconcile(All)`；`PAUSE_ON_FULLSCREEN` 与 5 个 `SHORTCUT_*` 原样保留
  - `carousel.rs`：本屏 + 同步模式从屏的 wallpaper_id 写入收进单事务，commit 后一次 reconcile
  - `deletion_effects.rs` `switch_wallpaper`：更新 + commit + reconcile(One)
  - `action_dispatch.rs`：只改 `is_enabled`（timer 字段，不进快照），**无需 reconcile**
- ✅ 前端停用：`monitorConfigStore.init()` 改为只读拉取；`upsert` / `upsertAll` / `syncConfigToAll` / `useMonitorSettings` 删除 `syncWallpaperWindows` 调用；`syncMonitors` 退化为 fetchConfigs

**Phase 3 剩余的处置（Phase 5 已收口前两项）**：
- ✅ `deletion_effects` 三处已改为「写 DB + 一次 reconcile」：helper 不再收集通知，公共入口统一调 `finish_deletion_reconcile()`；`RenderNotification` enum 与 `dispatch_notifications` 已删除；`on_wallpapers_deleted` 的无收藏夹分支补了 `clear_wallpaper_id` 落库（原实现只发通知不改 DB，留下 DB 指向已删壁纸的不一致）。
- ⬜ `set_setting` command 的 `app_setting_service::set` 尚未接入事务（单条写本身原子，display_mode 的批量 config 写已事务化）——用户决策不做，非阻塞。
// M20 已完成：sync_monitors_from_system 按三条规则统一登记新屏 / 失活断开屏。

### Phase 4：前端只读化（已落地，未 commit）

- ✅ `monitorConfigStore.init()` 退化为只读拉取（Phase 3 已做）；`syncMonitors` 退化为 `fetchConfigs`
- ✅ `upsert` / `upsertAll` / `syncConfigToAll` / `useMonitorSettings` 删除窗口编排（Phase 3 已做）
- ✅ 删除 `hooks/useMonitorHotPlug.ts`（5s 轮询）及 `App.tsx` 调用
- ✅ 删除 `stores/monitorSync.ts`（`doSyncMonitors` / `syncWallpaperWindows` / `activeWallpaperWindows`）——Phase 3 后已成孤儿
- ✅ 删除 `api/wallpaperWindow.ts`、`api/config.ts` 里 6 个窗口 command 常量 + CommandMap 项 + 2 个 DTO 接口
- ✅ 删除 Rust `commands/wallpaper_window.rs`、`dto/wallpaper_window_dto.rs` 及 mod/handler 注册
- ✅ 清理孤儿方法 `RenderCoordinator::active_ids()` / `WallpaperWindowManager::get_active_window_ids()`
// M20 已完成：useExtendViewport.ts 改为订阅 Rust 推送的 CSS 像素 viewport，不再本地计算

> 热插拔能力在 M20 落地前完全不响应（init 只在启动执行一次），这是明确的临时回归。

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

- **轮播定时器副作用**（`collection_id` / `play_mode` / `play_interval` / `is_enabled`）——归 Scheduler 现有路径，不进 Manager 快照、不由 reconcile 派发；本次只收编窗口调整副作用
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
Phase 1 ✅ 已落地（diff 引擎 + 9 组单测，纯逻辑零实机依赖）
  ↓
Phase 2 ✅ 已落地（Manager 骨架 + init + M18 DPI 换算，待 Windows 实机验收）
  ↓
Phase 2 → commit + Windows 实机（Manager + init + 窗口接管 + M18）
  ↓
Phase 3 ✅ 已落地（reconcile 派发 + 主要写入源收口 + 前端停用，待 Windows 实机验收）
  ↓
Phase 4 ✅ 已落地（前端编排与对外窗口 commands 全量删除；useExtendViewport 依赖 M20 未做）
  ↓
Phase 5 ✅ 已落地（deletion_effects 三处收口 + RenderNotification 下线 + M19 DeviceLost 清 WebView）
  ↓
M20 → Rust 热插拔事件监听 + 前端显示器参数只读 + useExtendViewport 改造（唯一剩余 P1 的落点）
```

---

## 7. 已确认（15:28）

1. **统一化**：所有 action（含 volume、普通 upsert、轮播 tick、删除联动）全部走「事务写 + 一次 `reconcile()`」，彻底取消旁路直接注入。✅
2. **scope 分级**：monitor 级 → 单 `monitor_id`；global 级 → 全部 `monitor_id`。✅ 实现上由事务写入的 monitor_id 集合推导，并覆盖 mirror/extend 的组级场景（见 2.1）。

### Phase 0 交付（已提交 `9a47d09`）

| 文件 | 改动 |
|---|---|
| `Cargo.toml` | `windows-sys` 加 `Win32_UI_HiDpi` feature |
| `monitor_geometry.rs` | 新增跨平台 `MonitorInfo { rect, dpi }` + `scale_factor()`；Windows 实现 `enumerate_monitors()`（`EnumDisplayMonitors` + `GetMonitorInfoW` + `GetDpiForMonitor(MDT_EFFECTIVE_DPI)`）；非 Windows stub 返回空集 |

验证：`cargo fmt --check` / `cargo check` / `cargo test --lib`（51 passed）全绿；Windows-only 代码用 `/tmp/monitor-geom-probe` 镜像整个模块跑 `cargo check --target x86_64-pc-windows-msvc`，**0 error / 0 warning**。

### Phase 1 交付（已提交 `52813ba` + 字段收敛修正）

| 文件 | 改动 |
|---|---|
| `ctx/reconcile.rs`（新） | `MonitorSnapshot`（**仅 `wallpaper_id` / `fit_mode` / `active` 三字段**）/ `GlobalSnapshot` / `ConfigSnapshot`；`ReconcileScope::{One, Many, All}` + `from_written_monitors()`；`ConfigDiff`（Added / Removed / Changed / DisplayModeChanged / VolumeChanged）；`diff_snapshots()`；`patch_observed()` |
| `ctx/mod.rs` | 注册 `pub mod reconcile` |

关键语义：
- `MonitorChanged` 把同屏多个字段变化合并成一项；`DisplayModeChanged` 独立成项，**不重复产出 set_fit_mode 等字段动作**（2.3 去重原则）。
- `patch_observed()` 只推进 `applied` 里的差异，绝不整体覆盖 observed——失败项下次 diff 会重放（C2 重试）、其它屏状态不受影响。
- scope 由事务写入的 monitor_id 集合推导，`All` 才比较 global 字段。

验证：`cargo fmt --check` / `cargo check`（0 warning）/ `cargo test --lib` **60 passed**（新增 9 组：三字段全覆盖、轮播换图经 wallpaper_id 捕获、scope 单屏隔离、批一次产出 N 屏、global 与字段动作去重、增删屏、失败重放、局部 patch 不动其它屏、scope 推导）。

### Phase 2 交付（已落地，未 commit）

| 文件 | 改动 |
|---|---|
| `ctx/unified_manager.rs`（新） | `MonitorSlot { rect, dpi, has_target }`；`UnifiedWallpaperManager`（组合持有 coordinator / window_manager / slots / observed）；`init()`；`build_snapshot()` 纯函数 |
| `ctx/mod.rs` | 注册模块、`AppContext` 增 `unified_manager` 字段、`new()` 里构造并 await `init()`（失败只记 error 不阻断启动） |
| `ctx/window_manager.rs` | **M18**：`logical_rect()` 把物理矩形按目标屏 `scale_factor` 换算为逻辑像素再传给 `WebviewWindowBuilder`；`monitor_scale_factor()` 从 `enumerate_monitors()` 取目标屏 DPI，非 Windows 回落 1.0；`embed_target` 仍保留物理坐标 |

验证：`cargo fmt --check` / `cargo check`（0 warning）/ `cargo test --lib` **65 passed**（新增 M18 换算 3 组、Manager 快照 2 组）。Windows-only 分支用 `/tmp/wm-dpi-probe`、`/tmp/um-win-probe` 分别跑 `cargo check --target x86_64-pc-windows-msvc`，**均 0 error / 0 warning**。

### Phase 3 交付（已落地，未 commit）

| 文件 | 改动 |
|---|---|
| `services/monitor_config_service.rs` | 写 API 泛型化到 `ConnectionTrait`；新增 `update_monitors_batch` |
| `services/wallpaper_service.rs` | `get_active_by_id` 泛型化 |
| `ctx/reconcile.rs` | `DispatchAction` + `plan_dispatch()` + 3 组单测 |
| `ctx/unified_manager.rs` | `reconcile(scope)` + `dispatch_one()` |
| `runtime/scheduler/mod.rs` | `unified_manager()` 访问器 |
| `runtime/scheduler/setting_effects.rs` | volume / display_mode 渲染副作用改走 reconcile；保留 timer 管理 |
| `runtime/scheduler/deletion_effects.rs` | switch_wallpaper 路径事务 + reconcile |
| `runtime/tasks/carousel.rs` | 轮播写入单事务 + commit 后 reconcile |
| `commands/monitor_config.rs` | upsert 的 4 条内联副作用 → 一次 reconcile |
| `stores/monitorConfigStore.ts` / `hooks/useMonitorSettings.ts` | 前端停用窗口编排，改为只读展示 |

验证：`cargo fmt --check` / `cargo check`（0 warning）/ `cargo test --lib` **68 passed**；前端 `npx tsc --noEmit` 与 `pnpm lint`（0 warning / 0 error）；Windows target 用镜像 probe（`um-win-probe`）验证 reconcile 全部派发方法类型正确，**0 error / 0 warning**。

**Phase 3 尚未完成的部分见 3 节末尾**，不把它算作已交付。待 Windows 实机验收后进入 Phase 4 前端彻底清理。

### Phase 5 交付（已落地，未 commit）

| 文件 | 改动 |
|---|---|
| `runtime/scheduler/deletion_effects.rs` | 三处公共入口改为「写 DB + 一次 `finish_deletion_reconcile()`」；helper 去掉 notifications 参数；`CurrentWallpaperOutcome::Advanced` 去掉 payload；无收藏夹分支补 `clear_wallpaper_id` 落库 |
| `ctx/render_coordinator.rs` | 删除 `RenderNotification` enum 与 `dispatch_notifications()` |
| `runtime/scheduler/mod.rs` | 删除 `render_coordinator()` 访问器（成为孤儿） |
| `ctx/window_manager.rs` | 新增 `destroy_all()`（M19） |
| `platform/windows/desktop_renderer/renderer.rs` | DeviceLost 分支 `tauri::async_runtime::spawn` 清理全部 WebView 壁纸窗口；补 `tauri::Manager` import |

验证：`cargo fmt/check`（0 warning）/ `test --lib` **68 passed**；前端 tsc + lint 全绿；Windows target 用 `/tmp/m19-probe` 验证 `state::<AppContext>()` → `window_manager.lock().destroy_all()` 类型链（**probe 抓到并修正了一处双重 lock bug**），0 error。

M19 不通知前端：前端显示器参数已降级为只读，窗口销毁后由下一次 `ensure_target` 命令重建；M20 事件化后统一处理。

---

## 附录：M20 评审（2026-11-23 19:37 确认）

> M20 是架构重构级任务，是**唯一剩余 P1（M18 WebView DPI）的落点**，也是三项挂起需求的汇集点。
> 用户已确认三个决策点：事件源 A（主窗口 WndProc 子类化）、建窗换算与 viewport 下发一起修、热插拔真正闭环。

### 根因

`useExtendViewport.ts` 用 `getCurrentWindow().scaleFactor()`（**本窗口** DPI）去除 `availableMonitors()` 返回的**物理像素** union。多屏不同 DPI 时，每个壁纸窗口用各自 scale 除整组坐标 → 接缝必然错位。

Native renderer 走的是另一条链：Win32 物理 rect → `union_rect` / `group_viewports` → 物理坐标裁剪。两条链各算各的，`render_coordinator.rs:28` 的注释已承认这个风险。

### 已具备的基础设施

| 设施 | 位置 | 状态 |
|---|---|---|
| `group_viewports()` 纯函数（物理坐标 union + 每屏 offset）+ 单测 | `platform/wallpaper_renderer/geometry.rs:56` | ✅ 可直接复用 |
| `ExtendViewport` 类型（camelCase serde，字段齐全） | `contracts.rs:170` | ✅ 可直接做事件 payload |
| WndProc 子类化框架 + 成对 register/restore | `desktop_embedder/wndproc.rs` | ✅ 需提升 `pub(crate)` 可见性 |
| 主窗口 HWND 获取 | `setup.rs:15` / `tray.rs:116` | ✅ 有先例 |

### 决策

| # | 决策点 | 结论 | 理由 |
|---|---|---|---|
| 1 | 事件源 | **主窗口 WndProc 子类化** | `subclass_wndproc` 常态路径是 `CallWindowProcW(original_proc, …)` 转发回 tao，`DefWindowProcW` 只是映射缺失的异常兜底。register/restore 成对时不存在"未注册窗口期"；主窗口是应用单例，卸载必定伴随注销 |
| 2 | 建窗换算 | **核实后确认无需改** | tao 的 `scale_factor()` 来自 `GetDpiForWindow`（窗口自身所在屏 DPI），而 `WebviewWindowBuilder::build()` 发生在嵌入之前，此时 `logical_rect()` 用目标屏 DPI 换算是对的。真正的 bug 只在嵌入后 WebView 侧 `scaleFactor()` 拿到主屏 DPI。**Phase 2 实现无需修改**，M20 只修 viewport 下发 |
| 3 | 热插拔闭环 | **真正闭环，自动写 DB** | active 变更对 Manager 有直接影响，Monitor 管理遵循下面三条规则 |

### Monitor 管理规则（用户给定，硬约束）

1. **初始化**：`init` 枚举到的 monitor 必定 `active=true`（DB 与 slot 同步登记）
2. **中途断开**：事件触发 → DB `active=false` → Manager `slots.remove(mid)` → `reconcile(One(mid))` 派发销毁
3. **中途插入**：事件触发 → DB `active=true`（新屏 insert config）→ Manager `slots.insert(mid, slot)` → `reconcile(One(mid))` 派发建窗

> 注意顺序：**先写 DB，再改 slot，最后 reconcile**。reconcile 从 DB 读 desired，若 slot 先于 DB 变更会得到错误的 diff 基线。

### 实施分期

| 期 | 内容 | 可验证性 |
|---|---|---|
| M20a ✅ | `ExtendViewportPayload`（目标屏 DPI 算好 CSS 像素）+ `get_extend_viewport` command + `useExtendViewport` 退化为订阅；`logical_rect()` 不变 | tsc/lint/68 tests 全绿 |
| M20b ✅ | 主窗口 WndProc 子类化监听 `WM_DISPLAYCHANGE` / `WM_DPICHANGED`；tokio unbounded channel + 300ms debounce；主窗口 Destroy 事件注销；WndProc 回调只 `send(())` | `m20-hotplug-probe` Windows target 0 error（抓到并修复 HWND 非 Send 的 static 错误） |
| M20c ✅ | `sync_monitors_from_system()` 实现三条规则；init 调用它（规则 1）；顺序固定 DB → slot → reconcile | `um-win-probe` Windows target 0 error |
| M20d ✅ | 联调验收：热插拔 + 多 DPI + extend 接缝 + 新屏自动建窗 | Windows 实机通过（用户 2026-11-23 确认） |

### 风险与处置

| 风险 | 处置 |
|---|---|
| 注销晚于 HWND 失效 | 挂主窗口 `Destroy` 事件，复用现有 `restore_and_unregister` |
| 同步 WndProc 回调不能 await / 查 DB | 回调只 `try_send` 到 mpsc，异步侧消费 |
| 插拔瞬间消息风暴 | 异步侧 300ms debounce 合并，再一次枚举 + reconcile |
| viewport 事件早于 WebView 建窗 | 事件按 `monitor_id` emit_to；新窗建好后由 init 路径补发一次当前 viewport |

### M20 实施记录（2026-11-23 19:43）

| 文件 | 改动 |
|---|---|
| `events.rs` | `ExtendViewportPayload` + `from_physical(ExtendViewport, dpi)`：用目标屏 DPI 把物理坐标换成 CSS 像素，前端零计算 |
| `ctx/unified_manager.rs` | 加 `app_handle`；`push_extend_viewports()` 按 label 定向推送；`current_extend_viewport()` 供挂载拉取；`sync_monitors_from_system()` 实现三条规则；init 改为调用它 |
| `commands/monitor_config.rs` | 新增 `get_extend_viewport` command |
| `ctx/window_manager.rs` | 新增 `wallpaper_window_label()`（建窗与事件推送共用命名） |
| `platform/windows/monitor_hotplug.rs`（新） | 主窗口 WndProc 子类化：只拦截 `WM_DISPLAYCHANGE` / `WM_DPICHANGED` 投信，其余 `CallWindowProcW` 转发；`start`/`stop` 幂等 |
| `platform/windows/desktop_embedder/wndproc.rs` | register/find/restore 提为 `pub(crate)` 供热插拔复用 |
| `setup.rs` | `start_monitor_hotplug()`：300ms debounce + 调 `sync_monitors_from_system()`；主窗口 Destroy 时注销 |
| 前端 `event.ts` / `config.ts` / `monitorConfig.ts` / `useExtendViewport.ts` | 订阅 `extend-viewport` + 挂载拉一次，删除 availableMonitors / scaleFactor 计算 |

**两处 probe 抓到并修复的真 bug**：
1. `hotplug_wndproc` 里把 `HWND`（裸指针、非 Send）存进 `static OnceLock` → 改为存 `isize`。
2. `SetWindowLongPtrW(..., hotplug_wndproc as isize)` 触发 `function_casts_as_integer` → 改为先转 `*const ()` 再转 `isize`。

验证：cargo fmt/check(0 warning)/test(68 passed)、tsc/lint 全绿；`m20-hotplug-probe` 与 `um-win-probe` Windows target 均 0 error。整项目 Windows check 仍被 `ring` 的 C 编译挡住（缺 `assert.h` / Windows SDK），用 probe 覆盖变更模块。

### 实机错误修复（2026-11-23 20:52，commit `9f27800`）

实机场景：一屏 Native + 一屏 WebView 混合，发现两个问题，均已修复：

| # | 现象 | 根因 | 修复 |
|---|---|---|---|
| 1 | WebView 屏保存视频壁纸后启动，Native 正常但 WebView 无渲染，应用卡 loading，终端卡在"寻找嵌入策略" | `AppContext::new` 在 setup 的 `block_on` 里同步 await `manager.init()`，其中 WebView 建窗 → `embed_in_desktop` 用 `SendMessageTimeoutW` 与 Progman/WorkerW 通信；setup 阶段主窗口事件循环未启动、消息无人处理 → 阻塞 | `init()` 从 `new()` 拆出为 `AppContext::init_manager()`，由 `setup::spawn_manager_init()` 在事件循环运行时异步调用。Native 不走嵌入，故只有 WebView 屏受影响，与现象吻合 |
| 2 | WebView 屏热插拔：拔掉→进程销毁，恢复→日志"已有 webview 窗口跳过创建"，该屏永不绘制 | `destroy_window` 的 `window.close()` 是异步的，Tauri 运行时同 label 实例尚未清理；`create_window` 检测到残留直接 `return Ok(None)` 跳过 | `create_window` 改 async：残留时主动再 close 一次，轮询等待运行时真正移除（最多 2s），超时返回 Err 让上层重试 |

验证：cargo fmt/check(0 warning)/test(68 passed)；um-win-probe clean 后 Windows target 0 error 0 warning（已同步生产代码，模块可见性镜像一致）。
