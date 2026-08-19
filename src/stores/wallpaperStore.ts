import { open } from "@tauri-apps/plugin-dialog";
import { create } from "zustand";
import type { Wallpaper } from "@/api/config";
import i18n from "@/i18n";
import {
  getAll as fetchAllWallpapers,
  importFiles as importWallpaperFiles,
  importWallpaperBytes,
  deleteBatch as deleteWallpaperBatch,
  getTrashed as fetchTrashedWallpapers,
  restoreBatch as restoreWallpaperBatch,
  purgeBatch as purgeWallpaperBatch,
  emptyTrash as emptyTrashApi,
  getSupportedExtensions as fetchSupportedExtensions,
  getById as fetchWallpaperById,
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
  /** 回收站内的壁纸（按移入时间倒序），仅回收站视图使用 */
  trashed: Wallpaper[];
  /** 回收站列表加载中 */
  trashLoading: boolean;

  fetchSupportedExtensions: () => Promise<string[]>;
  fetchWallpapers: () => Promise<void>;
  /** 根据 ID 列表精确刷新 store 中对应壁纸（不全量拉取） */
  refreshByIds: (ids: number[]) => Promise<void>;
  importWallpapers: () => Promise<void>;
  /**
   * 通过 H5 拖拽传入的 File[] 导入壁纸（字节方式，不依赖文件路径）
   * 返回 { imported, skipped, rejectedBySize }，便于上层提示
   */
  importByFiles: (files: File[]) => Promise<{
    imported: number;
    skipped: number;
    rejectedBySize: number;
  }>;
  /** 移入回收站（可恢复） */
  deleteWallpapers: (ids: number[]) => Promise<void>;
  /** 拉取回收站列表 */
  fetchTrashed: () => Promise<void>;
  /** 从回收站恢复（追加到原收藏夹末尾） */
  restoreWallpapers: (ids: number[]) => Promise<number>;
  /** 彻底删除（不可恢复） */
  purgeWallpapers: (ids: number[]) => Promise<number>;
  /** 清空回收站（不可恢复） */
  emptyTrash: () => Promise<number>;
}

/**
 * 对导入结果中的视频壁纸，分批（10 个一批）通过 canvas 抽取首帧缩略图，
 * 生成后调用后端持久化并精确刷新 store 中对应壁纸。
 */
async function generateVideoThumbnails(
  imported: Wallpaper[],
  refreshByIds: (ids: number[]) => Promise<void>,
) {
  const videoItems = imported
    .filter((w) => w.type === "video")
    .map((w) => ({ wallpaperId: w.id, filePath: w.file_path }));

  if (videoItems.length === 0) return;

  await batchExtractVideoThumbnails(videoItems, async (batchResults) => {
    // 逐个保存成功的缩略图
    const updatedIds: number[] = [];
    for (const { wallpaperId, data } of batchResults) {
      if (!data) continue;
      try {
        await saveVideoThumbnail(wallpaperId, data);
        updatedIds.push(wallpaperId);
      } catch (e) {
        console.error(`[VideoThumbnail] save failed for #${wallpaperId}`, e);
      }
    }
    // 每批完成后精确刷新已更新的壁纸，避免全量拉取
    if (updatedIds.length > 0) {
      await refreshByIds(updatedIds);
    }
  });
}

export const useWallpaperStore = create<WallpaperState>((set, get) => ({
  wallpapers: [],
  loading: false,
  supportedExtensions: [],
  trashed: [],
  trashLoading: false,

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

  refreshByIds: async (ids: number[]) => {
    try {
      const results = await Promise.all(ids.map((id) => fetchWallpaperById(id)));
      set((state) => {
        const updated = [...state.wallpapers];
        for (const wp of results) {
          if (!wp) continue;
          const idx = updated.findIndex((w) => w.id === wp.id);
          if (idx >= 0) updated[idx] = wp;
        }
        return { wallpapers: updated };
      });
    } catch (e) {
      console.error("[refreshByIds]", e);
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
      generateVideoThumbnails(imported, get().refreshByIds).catch((e) =>
        console.error("[VideoThumbnail] batch failed", e),
      );
    } catch (e) {
      console.error("[importWallpapers]", e);
    } finally {
      set({ loading: false });
    }
  },

  /**
   * 通过 H5 拖拽传入的 File[] 导入壁纸（字节方式，不依赖文件路径）
   *
   * 步骤：
   * 1. 按支持的扩展名过滤
   * 2. 体积守卫：单文件 > 200MB 或总量 > 500MB 直接拒绝（前端拒绝，不进 invoke）
   * 3. 逐文件串行：File → ArrayBuffer → Uint8Array，raw body 直传后端 import_wallpaper_bytes
   * 4. 复用现有 generateVideoThumbnails 异步分批生成视频缩略图
   */
  importByFiles: async (files: File[]) => {
    const extensions = await get().fetchSupportedExtensions();
    const extensionSet = new Set(extensions);

    // 体积上限（字节）
    const SINGLE_LIMIT = 200 * 1024 * 1024; // 单文件 200MB
    const TOTAL_LIMIT = 500 * 1024 * 1024; // 总量 500MB

    // 1) 扩展名过滤
    const matched: File[] = [];
    let skipped = 0;
    for (const f of files) {
      const ext = f.name.split(".").pop()?.toLowerCase() ?? "";
      if (extensionSet.has(ext)) {
        matched.push(f);
      } else {
        skipped += 1;
      }
    }

    if (matched.length === 0) {
      return { imported: 0, skipped, rejectedBySize: 0 };
    }

    // 2) 体积守卫：单文件超限直接剔除；剩余总量再次校验
    const sizeAccepted: File[] = [];
    let rejectedBySize = 0;
    let totalBytes = 0;
    for (const f of matched) {
      if (f.size > SINGLE_LIMIT) {
        rejectedBySize += 1;
        continue;
      }
      if (totalBytes + f.size > TOTAL_LIMIT) {
        rejectedBySize += 1;
        continue;
      }
      totalBytes += f.size;
      sizeAccepted.push(f);
    }

    if (sizeAccepted.length === 0) {
      return { imported: 0, skipped, rejectedBySize };
    }

    try {
      set({ loading: true });

      // 3) 逐文件串行导入：File → ArrayBuffer → Uint8Array，raw body 直传
      //    串行可将内存峰值控制在单个文件大小，且规避 SQLite 写锁竞争
      const imported: Wallpaper[] = [];
      for (const f of sizeAccepted) {
        try {
          const buf = await f.arrayBuffer();
          const wp = await importWallpaperBytes(f.name, new Uint8Array(buf));
          imported.push(wp);
        } catch (e) {
          console.error(`[DragImport] failed for ${f.name}`, e);
        }
      }
      console.log(`[DragImport] ${imported.length} wallpapers imported (bytes)`);

      await get().fetchWallpapers();

      // 4) 异步分批生成视频缩略图
      generateVideoThumbnails(imported, get().refreshByIds).catch((e) =>
        console.error("[VideoThumbnail] batch failed", e),
      );

      return { imported: imported.length, skipped, rejectedBySize };
    } catch (e) {
      console.error("[importByFiles]", e);
      return { imported: 0, skipped, rejectedBySize };
    } finally {
      set({ loading: false });
    }
  },

  deleteWallpapers: async (ids: number[]) => {
    try {
      const count = await deleteWallpaperBatch(ids);
      console.log(`[Trash] ${count} wallpapers moved to trash`);
      await get().fetchWallpapers();
      // 回收站列表已加载过时保持同步，避免用户切到回收站看到旧数据
      if (get().trashed.length > 0) {
        await get().fetchTrashed();
      }
    } catch (e) {
      console.error("[deleteWallpapers]", e);
    }
  },

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
      // 恢复影响两个列表：主网格新增、回收站减少
      await Promise.all([get().fetchWallpapers(), get().fetchTrashed()]);
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
      await get().fetchTrashed();
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
      await get().fetchTrashed();
      return count;
    } catch (e) {
      console.error("[emptyTrash]", e);
      return 0;
    }
  },
}));