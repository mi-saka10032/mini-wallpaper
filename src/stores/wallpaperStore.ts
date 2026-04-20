import { open } from "@tauri-apps/plugin-dialog";
import { create } from "zustand";
import type { Wallpaper } from "@/api/config";
import {
  getAll as fetchAllWallpapers,
  importFiles as importWallpaperFiles,
  deleteBatch as deleteWallpaperBatch,
} from "@/api/wallpaper";

// 从 config 中 re-export Wallpaper 类型，方便外部使用
export type { Wallpaper } from "@/api/config";

/** 支持的壁纸文件扩展名 */
const SUPPORTED_EXTENSIONS = new Set([
  "jpg", "jpeg", "png", "bmp", "webp", "gif",
  "mp4", "webm", "mkv", "avi", "mov",
]);

interface WallpaperState {
  wallpapers: Wallpaper[];
  loading: boolean;

  fetchWallpapers: () => Promise<void>;
  importWallpapers: () => Promise<void>;
  importByPaths: (paths: string[]) => Promise<void>;
  deleteWallpapers: (ids: number[]) => Promise<void>;
}

export const useWallpaperStore = create<WallpaperState>((set, get) => ({
  wallpapers: [],
  loading: false,

  fetchWallpapers: async () => {
    try {
      const list = await fetchAllWallpapers();
      set({ wallpapers: list });
    } catch (e) {
      console.error("[fetchWallpapers]", e);
    }
  },

  importWallpapers: async () => {
    try {
      const selected = await open({
        multiple: true,
        filters: [
          {
            name: "壁纸文件",
            extensions: [...SUPPORTED_EXTENSIONS],
          },
        ],
      });

      if (!selected || selected.length === 0) return;

      set({ loading: true });

      const paths = selected as string[];
      const imported = await importWallpaperFiles(paths);
      console.log(`[Import] ${imported.length} wallpapers imported`);

      await get().fetchWallpapers();
    } catch (e) {
      console.error("[importWallpapers]", e);
    } finally {
      set({ loading: false });
    }
  },

  /** 通过路径数组直接导入（拖拽导入使用） */
  importByPaths: async (paths: string[]) => {
    // 过滤出支持的文件格式
    const validPaths = paths.filter((p) => {
      const ext = p.split(".").pop()?.toLowerCase() ?? "";
      return SUPPORTED_EXTENSIONS.has(ext);
    });
    if (validPaths.length === 0) return;

    try {
      set({ loading: true });
      const imported = await importWallpaperFiles(validPaths);
      console.log(`[DragImport] ${imported.length} wallpapers imported`);
      await get().fetchWallpapers();
    } catch (e) {
      console.error("[importByPaths]", e);
    } finally {
      set({ loading: false });
    }
  },

  deleteWallpapers: async (ids: number[]) => {
    try {
      const count = await deleteWallpaperBatch(ids);
      console.log(`[Delete] ${count} wallpapers deleted`);
      await get().fetchWallpapers();
    } catch (e) {
      console.error("[deleteWallpapers]", e);
    }
  },
}));
