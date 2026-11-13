import { create } from "zustand";
import type { Wallpaper } from "@/api/config";
import {
  getTrashed as fetchTrashedWallpapers,
  restoreBatch as restoreWallpaperBatch,
  purgeBatch as purgeWallpaperBatch,
  emptyTrash as emptyTrashApi,
} from "@/api/wallpaper";
import { useFavoritesStore } from "@/stores/favoritesStore";
import { useWallpaperStore } from "@/stores/wallpaperStore";

/**
 * 回收站 store
 *
 * 与 wallpaperStore 的边界：后者负责「正常壁纸」视图（主网格 / 收藏夹 / 智能夹），
 * 本 store 只负责回收站视图。移入回收站由 wallpaperStore.deleteWallpapers 发起
 * （数据从它那边流出），恢复 / 彻底删除 / 清空由本 store 发起。
 *
 * 跨 store 调用一律走 `useXxxStore.getState()`，依赖方向单向：
 * trashedStore → wallpaperStore / favoritesStore，不存在环。
 */
interface TrashedState {
  /** 回收站内的壁纸（按移入时间倒序） */
  trashed: Wallpaper[];
  /** 列表加载中 */
  trashLoading: boolean;

  /** 拉取回收站列表 */
  fetchTrashed: () => Promise<void>;
  /** 从回收站恢复（追加到原收藏夹末尾），返回恢复数量 */
  restoreWallpapers: (ids: number[]) => Promise<number>;
  /** 彻底删除（不可恢复），返回删除数量 */
  purgeWallpapers: (ids: number[]) => Promise<number>;
  /** 清空回收站（不可恢复），返回删除数量 */
  emptyTrash: () => Promise<number>;
}

export const useTrashedStore = create<TrashedState>((set, get) => ({
  trashed: [],
  trashLoading: false,

  fetchTrashed: async () => {
    try {
      set({ trashLoading: true });
      const list = await fetchTrashedWallpapers();
      set({ trashed: list });
    } catch (e) {
      console.error("[fetchTrashed]", e);
    } finally {
      set({ trashLoading: false });
    }
  },

  restoreWallpapers: async (ids: number[]) => {
    try {
      const count = await restoreWallpaperBatch(ids);
      console.log(`[Trash] ${count} wallpapers restored`);
      // 恢复影响两侧：回收站减少、正常壁纸增加。
      // 操作发生在回收站视图内（activeId 为负），refreshView 会 early return，
      // 所以必须显式调 fetchWallpapers 更新基础列表；
      // refreshView 仍然要调，覆盖「用户在别处恢复」的场景（智能夹是实时求值的，
      // 恢复的壁纸可能重新命中规则）。
      await Promise.all([
        get().fetchTrashed(),
        useWallpaperStore.getState().fetchWallpapers(),
        useWallpaperStore.getState().refreshView(),
      ]);
      return count;
    } catch (e) {
      console.error("[restoreWallpapers]", e);
      return 0;
    }
  },

  purgeWallpapers: async (ids: number[]) => {
    try {
      const count = await purgeWallpaperBatch(ids);
      console.log(`[Trash] ${count} wallpapers permanently deleted`);
      // 彻底删除会级联清掉收藏夹关系，而 favoriteIds 只在启动时初始化，
      // 不重取会残留已删除的 id（SQLite 复用 id 时会让新壁纸错亮红心）
      await Promise.all([get().fetchTrashed(), useFavoritesStore.getState().refresh()]);
      return count;
    } catch (e) {
      console.error("[purgeWallpapers]", e);
      return 0;
    }
  },

  emptyTrash: async () => {
    try {
      const count = await emptyTrashApi();
      console.log(`[Trash] emptied, ${count} wallpapers permanently deleted`);
      await Promise.all([get().fetchTrashed(), useFavoritesStore.getState().refresh()]);
      return count;
    } catch (e) {
      console.error("[emptyTrash]", e);
      return 0;
    }
  },
}));
