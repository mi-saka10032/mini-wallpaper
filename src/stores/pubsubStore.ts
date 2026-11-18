import { create } from "zustand";
import { COMMANDS } from "@/api/config";

/**
 * 数据失效信号的发布订阅商店。
 *
 * 后端持有权威数据，前端这四份都是快照：
 * - `wallpapers` / `viewWallpapers`：全局基础缓存 + 当前视图成员
 *   （「我喜欢」走关联表、智能夹走规则引擎实时求值）
 * - `favoriteIds`：红心亮灭
 * - `trashed`：回收站列表（TrashView 常驻挂载，切走仅隐藏，不会自动重取）
 * - `collections`：侧边栏收藏夹列表（名称 / 规则 / 数量）
 *
 * 写命令成功时由 `invoke` 统一发布信号，store 不再手动调用任何 fetch / refresh；
 * 四类信号分开递增，订阅方只重取真正受影响的那份数据；
 * 判定是「命令 → 信号集合」的静态映射，不依赖当前视图类型等运行时状态，
 * 避免为了省几次本地 IPC 引入易漏判的动态规则（漏判就会让列表不更新）。
 *
 * 订阅与注销复用 zustand 的 `subscribe`，`subscribe` 返回的注销函数由
 * React effect cleanup 接住，StrictMode / HMR 不重复注册。
 *
 * 本模块只依赖 `zustand` + `api/config` 常量，**不 import 任何业务 store**，
 * 否则 `invoke -> pubsubStore -> wallpaperStore -> api/collection -> invoke` 成环。
 */

/**
 * 需要重取的数据源。
 *
 * 用 `as const` 对象 + 同源类型提取替代 `enum`：值命名空间提供
 * `Signal.VIEW` 这样的枚举式访问，类型命名空间仍是字面量联合。
 * 不生成运行时 enum 对象，也不受 `isolatedModules` 下 `const enum` 的限制。
 * 项目里的 `EVENTS` / `SETTING_KEYS` 也是同一种写法。
 */
export const Signal = {
  VIEW: "view",
  FAVORITES: "favorites",
  TRASH: "trash",
  COLLECTIONS: "collections",
} as const;

export type Signal = (typeof Signal)[keyof typeof Signal];

/**
 * 命令 → 受影响的信号。集中一处维护，新增入口只要走 invoke 就自动发布。
 * 删除 / 恢复 / 彻底删除 / 清空同时改动回收站与正常壁纸，故三类信号都发；
 * 收藏夹成员增删保守地带上 `favorites`（目标可能是内置「我喜欢」）；
 * 收藏夹自身的增删改另发 `collections` 供侧边栏与规则编辑回填。
 */
const COMMAND_SIGNALS: Record<string, readonly Signal[]> = {
  [COMMANDS.TOGGLE_FAVORITE]: [Signal.VIEW, Signal.FAVORITES],
  [COMMANDS.ADD_WALLPAPERS_TO_COLLECTION]: [Signal.VIEW, Signal.FAVORITES],
  [COMMANDS.REMOVE_WALLPAPERS_FROM_COLLECTION]: [Signal.VIEW, Signal.FAVORITES],
  [COMMANDS.REORDER_COLLECTION_WALLPAPERS]: [Signal.VIEW],
  [COMMANDS.CREATE_COLLECTION]: [Signal.COLLECTIONS],
  [COMMANDS.RENAME_COLLECTION]: [Signal.COLLECTIONS],
  [COMMANDS.DELETE_COLLECTION]: [Signal.VIEW, Signal.COLLECTIONS],
  [COMMANDS.REORDER_COLLECTIONS]: [Signal.COLLECTIONS],
  [COMMANDS.CREATE_SMART_COLLECTION]: [Signal.VIEW, Signal.COLLECTIONS],
  [COMMANDS.UPDATE_SMART_COLLECTION]: [Signal.VIEW, Signal.COLLECTIONS],
  [COMMANDS.SET_WALLPAPER_TAGS]: [Signal.VIEW],
  [COMMANDS.TAG_WALLPAPERS]: [Signal.VIEW],
  [COMMANDS.UNTAG_WALLPAPERS]: [Signal.VIEW],
  [COMMANDS.RENAME_TAG]: [Signal.VIEW],
  [COMMANDS.DELETE_TAG]: [Signal.VIEW],
  [COMMANDS.IMPORT_WALLPAPERS]: [Signal.VIEW],
  [COMMANDS.IMPORT_WALLPAPER_BYTES]: [Signal.VIEW],
  [COMMANDS.DELETE_WALLPAPERS]: [Signal.VIEW, Signal.FAVORITES, Signal.TRASH],
  [COMMANDS.RESTORE_WALLPAPERS]: [Signal.VIEW, Signal.FAVORITES, Signal.TRASH],
  [COMMANDS.PURGE_WALLPAPERS]: [Signal.VIEW, Signal.FAVORITES, Signal.TRASH],
  [COMMANDS.EMPTY_TRASH]: [Signal.VIEW, Signal.FAVORITES, Signal.TRASH],
};

interface PubSubState {
  viewVersion: number;
  favoritesVersion: number;
  trashVersion: number;
  collectionsVersion: number;
  /** 一次 set 内推进多个版本，保证订阅者只收到一次通知 */
  bump: (signals: readonly Signal[]) => void;
}

export const usePubSubStore = create<PubSubState>((set) => ({
  viewVersion: 0,
  favoritesVersion: 0,
  trashVersion: 0,
  collectionsVersion: 0,
  bump: (signals) =>
    set((state) => ({
      viewVersion: state.viewVersion + (signals.includes(Signal.VIEW) ? 1 : 0),
      favoritesVersion: state.favoritesVersion + (signals.includes(Signal.FAVORITES) ? 1 : 0),
      trashVersion: state.trashVersion + (signals.includes(Signal.TRASH) ? 1 : 0),
      collectionsVersion: state.collectionsVersion + (signals.includes(Signal.COLLECTIONS) ? 1 : 0),
    })),
}));

/** 发布一组信号（供不经过 invoke 的来源使用，如后端事件） */
export function publish(signals: readonly Signal[]): void {
  usePubSubStore.getState().bump(signals);
}

/** invoke 成功出口调用：仅映射表内的写命令发布信号 */
export function publishCommandSignals(command: string): void {
  const signals = COMMAND_SIGNALS[command];
  if (signals) usePubSubStore.getState().bump(signals);
}
