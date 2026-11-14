import { create } from "zustand";
import {
  getAll as fetchAllCollections,
  getCollectionWallpapers,
  toggleFavorite as toggleFavoriteApi,
} from "@/api/collection";
import { useCollectionStore } from "@/stores/collectionStore";

/**
 * 收藏状态 store
 *
 * 维护内置「我喜欢」收藏夹的 id 与其中壁纸 id 集合，供红心按钮亮灭判断与切换。
 * 后端 `favorites-changed` 事件（快捷键 / 托盘 / 红心按钮触发）会回流到此 store，
 * 保证三方入口的收藏状态实时一致。
 */
interface FavoritesState {
  /** 内置收藏夹 id（未初始化时为 null） */
  builtinCollectionId: number | null;
  /** 已收藏的壁纸 id 集合 */
  favoriteIds: Set<number>;
  /** 初始化：定位内置收藏夹并拉取其壁纸 id 集合 */
  init: () => Promise<void>;
  /**
   * 重新拉取收藏 id 集合（复用已知的 builtinCollectionId，比 init 少一次 IPC）
   *
   * 删除类操作不会经过 toggle，也没有 favorites-changed 事件回流，
   * 局部 patch 容易漏（彻底删除要清、软删除恢复后又要留），所以直接重取权威数据。
   */
  refresh: () => Promise<void>;
  /** 切换收藏状态（调用后端，返回切换后的状态） */
  toggle: (wallpaperId: number) => Promise<boolean>;
  /** 本地应用一次收藏状态变更（供事件回流复用，不触发后端） */
  applyChange: (wallpaperId: number, favorited: boolean) => void;
}

export const useFavoritesStore = create<FavoritesState>((set, get) => ({
  builtinCollectionId: null,
  favoriteIds: new Set(),

  init: async () => {
    try {
      // App 的启动编排会先拉好收藏夹列表并填入 collectionStore，这里直接复用，
      // 避免启动期同一个 getAll 被调用两次。单独调用本方法时兜底自行拉取。
      let collections = useCollectionStore.getState().collections;
      if (collections.length === 0) {
        collections = await fetchAllCollections();
      }
      const builtin = collections.find((c) => c.is_builtin === 1);
      if (!builtin) return;
      const wallpapers = await getCollectionWallpapers(builtin.id);
      set({
        builtinCollectionId: builtin.id,
        favoriteIds: new Set(wallpapers.map((w) => w.id)),
      });
    } catch (e) {
      console.error("[favoritesStore.init]", e);
    }
  },

  refresh: async () => {
    const collectionId = get().builtinCollectionId;
    // 尚未初始化时退回 init，避免静默什么都不做
    if (collectionId === null) {
      await get().init();
      return;
    }
    try {
      const wallpapers = await getCollectionWallpapers(collectionId);
      set({ favoriteIds: new Set(wallpapers.map((w) => w.id)) });
    } catch (e) {
      console.error("[favoritesStore.refresh]", e);
    }
  },

  toggle: async (wallpaperId: number) => {
    const favorited = await toggleFavoriteApi(wallpaperId);
    get().applyChange(wallpaperId, favorited);
    return favorited;
  },

  applyChange: (wallpaperId: number, favorited: boolean) => {
    set((state) => {
      const next = new Set(state.favoriteIds);
      if (favorited) next.add(wallpaperId);
      else next.delete(wallpaperId);
      return { favoriteIds: next };
    });
  },
}));
