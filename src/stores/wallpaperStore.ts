import { open } from "@tauri-apps/plugin-dialog";
import { create } from "zustand";
import type { Wallpaper } from "@/api/config";
import i18n from "@/i18n";
import {
  getAll as fetchAllWallpapers,
  importFiles as importWallpaperFiles,
  deleteBatch as deleteWallpaperBatch,
  getSupportedExtensions as fetchSupportedExtensions,
  saveVideoThumbnail,
} from "@/api/wallpaper";
import { batchExtractVideoThumbnails } from "@/lib/videoThumbnail";

// 从 config 中 re-export Wallpaper 类型，方便外部使用
export type { Wallpaper } from "@/api/config";

interface WallpaperState {
  wallpapers: Wallpaper[];
  loading: boolean;
  /** 后端返回的支持扩展名列表（懒加载缓存） */
  supportedExtensions: string[];

  fetchSupportedExtensions: () => Promise<string[]>;
  fetchWallpapers: () => Promise<void>;
  importWallpapers: () => Promise<void>;
  importByPaths: (paths: string[]) => Promise<void>;
  deleteWallpapers: (ids: number[]) => Promise<void>;
}

/**
 * 对导入结果中的视频壁纸，分批（10 个一批）通过 canvas 抽取首帧缩略图，
 * 生成后调用后端持久化并刷新 store。
 */
async function generateVideoThumbnails(
  imported: Wallpaper[],
  refreshWallpapers: () => Promise<void>,
) {
  const videoItems = imported
    .filter((w) => w.type === "video")
    .map((w) => ({ wallpaperId: w.id, filePath: w.file_path }));

  if (videoItems.length === 0) return;

  await batchExtractVideoThumbnails(videoItems, async (batchResults) => {
    // 逐个保存成功的缩略图
    for (const { wallpaperId, data } of batchResults) {
      if (!data) continue;
      try {
        await saveVideoThumbnail(wallpaperId, Array.from(data));
      } catch (e) {
        console.error(`[VideoThumbnail] save failed for #${wallpaperId}`, e);
      }
    }
    // 每批完成后刷新列表，让 UI 逐步显示缩略图
    await refreshWallpapers();
  });
}

export const useWallpaperStore = create<WallpaperState>((set, get) => ({
  wallpapers: [],
  loading: false,
  supportedExtensions: [],

  /** 获取支持的扩展名（带缓存，仅首次调用时请求后端） */
  fetchSupportedExtensions: async () => {
    const cached = get().supportedExtensions;
    if (cached.length > 0) return cached;

    try {
      const extensions = await fetchSupportedExtensions();
      set({ supportedExtensions: extensions });
      return extensions;
    } catch (e) {
      console.error("[fetchSupportedExtensions]", e);
      return [];
    }
  },

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
      const extensions = await get().fetchSupportedExtensions();
      if (extensions.length === 0) return;

      const selected = await open({
        multiple: true,
        filters: [
          {
            name: i18n.t("main.wallpaperFiles"),
            extensions,
          },
        ],
      });

      if (!selected || selected.length === 0) return;

      set({ loading: true });

      const paths = selected as string[];
      const imported = await importWallpaperFiles(paths);
      console.log(`[Import] ${imported.length} wallpapers imported`);

      // 先刷新列表（视频壁纸此时 thumb_path 为 null，显示占位图）
      await get().fetchWallpapers();

      // 异步分批生成视频缩略图（不阻塞 loading 状态）
      generateVideoThumbnails(imported, get().fetchWallpapers).catch((e) =>
        console.error("[VideoThumbnail] batch failed", e),
      );
    } catch (e) {
      console.error("[importWallpapers]", e);
    } finally {
      set({ loading: false });
    }
  },

  /** 通过路径数组直接导入（拖拽导入使用） */
  importByPaths: async (paths: string[]) => {
    const extensions = await get().fetchSupportedExtensions();
    const extensionSet = new Set(extensions);

    // 过滤出支持的文件格式
    const validPaths = paths.filter((p) => {
      const ext = p.split(".").pop()?.toLowerCase() ?? "";
      return extensionSet.has(ext);
    });
    if (validPaths.length === 0) return;

    try {
      set({ loading: true });
      const imported = await importWallpaperFiles(validPaths);
      console.log(`[DragImport] ${imported.length} wallpapers imported`);

      await get().fetchWallpapers();

      // 异步分批生成视频缩略图
      generateVideoThumbnails(imported, get().fetchWallpapers).catch((e) =>
        console.error("[VideoThumbnail] batch failed", e),
      );
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
