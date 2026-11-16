import { create } from "zustand";
import type { Wallpaper } from "@/api/config";
import {
  getTrashed as fetchTrashedWallpapers,
  restoreBatch as restoreWallpaperBatch,
  purgeBatch as purgeWallpaperBatch,
  emptyTrash as emptyTrashApi,
} from "@/api/wallpaper";
/**
 * 回收站 store
 *
 * 与 wallpaperStore 的边界：后者负责「正常壁纸」视图（主网格 / 收藏夹 / 智能夹），
 * 本 store 只负责回收站视图。移入回收站由 wallpaperStore.deleteWallpapers 发起
 * （数据从它那边流出），恢复 / 彻底删除 / 清空由本 store 发起。
 *
 * 所有快照重取（视图 / 红心 / 回收站 / 收藏夹列表）都由 invoke 失效信号
 * 经 pub/sub 订阅方统一驱动，本 store 的动作只做写操作、返回数量。
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

export const useTrashedStore = create<TrashedState>((set) => ({
  trashed: [],
  trashLoading: false,

  fetchTrashed: async () => {
    try {
      set({ trashLoading: true });
      const list = await fetchTrashedWallpapers();
      set({ trashed: list });
    } catch (e) {
      // 尽力刷新：由 pub/sub 订阅方 fire-and-forget 调用，失败时 invoke 层
      // 已统一弹提示；这里不上抛，避免未处理的 rejection 打断其它信号的重取。
      console.error("[fetchTrashed]", e);
    } finally {
      set({ trashLoading: false });
    }
  },

  restoreWallpapers: async (ids: number[]) => {
    const count = await restoreWallpaperBatch(ids);
    console.log(`[Trash] ${count} wallpapers restored`);
    // 三份快照由 invoke 失效信号统一重取；负值视图下 sync() 仍会更新基础缓存
    return count;
  },

  purgeWallpapers: async (ids: number[]) => {
    const count = await purgeWallpaperBatch(ids);
    console.log(`[Trash] ${count} wallpapers permanently deleted`);
    // 回收站列表与 favoriteIds 由 invoke 失效信号统一重取：
    // 彻底删除会级联清掉收藏夹关系，不重取会残留已删除的 id
    // （SQLite 复用 id 时会让新壁纸错亮红心）
    return count;
  },

  emptyTrash: async () => {
    const count = await emptyTrashApi();
    console.log(`[Trash] emptied, ${count} wallpapers permanently deleted`);
    // 同 purgeWallpapers：回收站列表与红心集合由失效信号重取
    return count;
  },
}));
