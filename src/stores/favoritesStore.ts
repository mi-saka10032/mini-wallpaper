import { create } from "zustand";
import { getAll as fetchAllCollections, getCollectionWallpapers, toggleFavorite as toggleFavoriteApi } from "@/api/collection";

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
  /** 判断某壁纸是否已收藏 */
  isFavorite: (wallpaperId: number) => boolean;
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
      const collections = await fetchAllCollections();
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

  isFavorite: (wallpaperId: number) => get().favoriteIds.has(wallpaperId),

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
