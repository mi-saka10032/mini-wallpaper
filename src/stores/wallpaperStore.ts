import { open } from "@tauri-apps/plugin-dialog";
import { create } from "zustand";
import type { Wallpaper } from "@/api/config";
import i18n from "@/i18n";
import { getCollectionWallpapers } from "@/api/collection";
import {
  getAll as fetchAllWallpapers,
  importFiles as importWallpaperFiles,
  importWallpaperBytes,
  deleteBatch as deleteWallpaperBatch,
  getSupportedExtensions as fetchSupportedExtensions,
  getById as fetchWallpaperById,
  saveVideoThumbnail,
} from "@/api/wallpaper";
import { batchExtractVideoThumbnails } from "@/lib/videoThumbnail";
import { useTrashedStore } from "@/stores/trashedStore";

// 从 config 中 re-export Wallpaper 类型，方便外部使用
export type { Wallpaper } from "@/api/config";

interface WallpaperState {
  /** 全部正常壁纸（activeId === 0 时即当前视图） */
  wallpapers: Wallpaper[];
  loading: boolean;
  /** 后端返回的支持扩展名列表（懒加载缓存） */
  supportedExtensions: string[];

  /**
   * 当前视图标识：0 = 本地壁纸，>0 = 收藏夹 id（普通/智能/「我喜欢」统一走后端分流），
   * 负值为非壁纸视图（如 -1 显示器设置、TRASH_VIEW_ID 回收站），此时不拉取。
   */
  activeId: number;
  /** 当前视图的壁纸列表 */
  viewWallpapers: Wallpaper[];
  /** 视图加载中 */
  viewLoading: boolean;

  fetchSupportedExtensions: () => Promise<string[]>;
  fetchWallpapers: () => Promise<void>;
  /** 根据 ID 列表精确刷新 store 中对应壁纸（不全量拉取） */
  refreshByIds: (ids: number[]) => Promise<void>;

  /** 切换视图并拉取对应数据（内部带竞态守卫） */
  setActiveId: (id: number) => Promise<void>;
  /**
   * 按当前 activeId 重新拉取视图数据
   *
   * 任何改变壁纸归属的操作（导入 / 删除 / 恢复 / 收藏夹增删）完成后都应调用它。
   * 智能夹是后端实时求值的，导入或打标签都可能改变其命中集，
   * 所以不能只刷新 activeId === 0 的分支。
   */
  refreshView: () => Promise<void>;

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

/**
 * 视图请求序号：每次发起视图拉取自增，响应回来时比对。
 *
 * 快速切换收藏夹 A→B 时，A 的响应可能后到并覆盖 B 的数据（串台）。
 * 放在模块作用域而非 store state，是因为它纯属内部簿记，
 * 放进 state 会让每次自增都触发订阅者重渲染。
 */
let viewRequestSeq = 0;

/** 按 activeId 取对应数据源。负值视图（显示器设置 / 回收站）不由本 store 负责 */
async function fetchViewData(activeId: number): Promise<Wallpaper[] | null> {
  if (activeId < 0) return null;
  if (activeId === 0) return fetchAllWallpapers();
  // 普通夹 / 智能夹 / 「我喜欢」统一走此命令，后端按 kind 分流
  return getCollectionWallpapers(activeId);
}

export const useWallpaperStore = create<WallpaperState>((set, get) => ({
  wallpapers: [],
  loading: false,
  supportedExtensions: [],
  activeId: 0,
  viewWallpapers: [],
  viewLoading: false,

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
      // activeId === 0 时 wallpapers 就是当前视图，同步更新避免两份数据不一致
      set((state) =>
        state.activeId === 0 ? { wallpapers: list, viewWallpapers: list } : { wallpapers: list },
      );
    } catch (e) {
      console.error("[fetchWallpapers]", e);
    }
  },

  setActiveId: async (id: number) => {
    if (get().activeId === id) return;
    set({ activeId: id });
    await get().refreshView();
  },

  refreshView: async () => {
    const activeId = get().activeId;
    const seq = ++viewRequestSeq;

    if (activeId < 0) return;

    set({ viewLoading: true });
    try {
      const list = await fetchViewData(activeId);
      // 响应回来时若已不是最新请求（用户又切了视图），丢弃本次结果
      if (seq !== viewRequestSeq) return;
      if (list === null) return;
      set(activeId === 0 ? { wallpapers: list, viewWallpapers: list } : { viewWallpapers: list });
    } catch (e) {
      console.error("[refreshView]", e);
    } finally {
      // 同样受守卫约束：过期请求不应关掉新请求的 loading
      if (seq === viewRequestSeq) set({ viewLoading: false });
    }
  },

  refreshByIds: async (ids: number[]) => {
    try {
      const results = await Promise.all(ids.map((id) => fetchWallpaperById(id)));
      // 同一壁纸对象可能同时存在于 wallpapers 与 viewWallpapers（收藏夹视图），
      // 两处都要替换，否则缩略图在其中一个视图里不更新
      const replaceIn = (list: Wallpaper[]) => {
        const updated = [...list];
        for (const wp of results) {
          if (!wp) continue;
          const idx = updated.findIndex((w) => w.id === wp.id);
          if (idx >= 0) updated[idx] = wp;
        }
        return updated;
      };
      set((state) => ({
        wallpapers: replaceIn(state.wallpapers),
        viewWallpapers: replaceIn(state.viewWallpapers),
      }));
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
      // 用 refreshView 而非 fetchWallpapers：用户可能正停在智能夹视图上，
      // 而智能夹是后端实时求值的，新导入的壁纸可能直接命中规则
      await Promise.all([get().fetchWallpapers(), get().refreshView()]);

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

      await Promise.all([get().fetchWallpapers(), get().refreshView()]);

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
      // 当前视图减少、回收站增加。回收站列表由 trashedStore 持有，
      // 这里主动同步是因为 TrashView 常驻挂载（切走仅隐藏），
      // 不会在切回时重新拉取。
      await Promise.all([
        get().fetchWallpapers(),
        get().refreshView(),
        useTrashedStore.getState().fetchTrashed(),
      ]);
    } catch (e) {
      console.error("[deleteWallpapers]", e);
      throw e;
    }
  },
}));
