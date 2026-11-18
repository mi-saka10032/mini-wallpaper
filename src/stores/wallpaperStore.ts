import { open } from "@tauri-apps/plugin-dialog";
import { create } from "zustand";
import { COMMANDS, type Wallpaper } from "@/api/config";
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
import { publishCommandSignals } from "@/stores/pubsubStore";

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
   * 失效后统一对齐本 store 的两份快照：全局 `wallpapers` 基础缓存
   * 与当前 `viewWallpapers` 视图快照。只由 pub/sub 订阅方调用，
   * 业务 action 不自行触发任何 fetch / refresh。
   */
  sync: () => Promise<void>;
  /** 按当前 activeId 重新拉取视图数据（负值视图直接返回） */
  refreshView: () => Promise<void>;

  /** 通过系统文件选择器导入（失败时上抛，由调用方提示） */
  importWallpapers: () => Promise<ImportResult>;
  /**
   * 按文件路径导入壁纸（Tauri 原生拖放场景）
   *
   * 与 importByFiles 的区别：后者依赖 WebView 读出文件字节，而 WebView2 在
   * 生产构建（tauri:// 自定义协议）下会给出 size 正常但读不出内容的 File 对象。
   * 原生拖放事件直接给路径，由后端自行读盘，与点击导入走同一条已验证的链路。
   */
  importByPaths: (paths: string[]) => Promise<ImportResult>;
  /**
   * 通过 H5 拖拽传入的 File[] 导入壁纸（字节方式，不依赖文件路径）
   */
  importByFiles: (files: File[]) => Promise<ImportResult>;
  /** 移入回收站（可恢复） */
  deleteWallpapers: (ids: number[]) => Promise<void>;
}

/**
 * 三条导入路径（文件选择器 / 原生拖放 / HTML5 拖拽）统一的返回形状。
 *
 * `cancelled` 区分「用户主动取消选择」与「选了但全部失败」：
 * 前者不该弹任何提示，后者必须报错。之前 importWallpapers 返回 void、
 * 其余两条返回计数，调用方无从统一判断。
 */
export interface ImportResult {
  /** 成功导入的数量 */
  imported: number;
  /** 因扩展名不受支持而跳过的数量 */
  skipped: number;
  /** 因体积超限而跳过的数量（仅字节通道可能非零） */
  rejectedBySize: number;
  /** 用户取消了文件选择（仅 importWallpapers 可能为 true） */
  cancelled: boolean;
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

/** 未导入任何文件时的中性结果（各字段为 0、未取消） */
const EMPTY_IMPORT_RESULT: ImportResult = {
  imported: 0,
  skipped: 0,
  rejectedBySize: 0,
  cancelled: false,
};

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

  /**
   * 获取支持的扩展名（带缓存，仅首次调用时请求后端）。
   * 失败直接上抛，交给三个导入 action 各自的反馈层提示；返回空数组会让它们
   * 静默返回「没有可导入的文件」，与真实错误相互矛盾。
   */
  fetchSupportedExtensions: async () => {
    const cached = get().supportedExtensions;
    if (cached.length > 0) return cached;

    const extensions = await fetchSupportedExtensions();
    set({ supportedExtensions: extensions });
    return extensions;
  },

  fetchWallpapers: async () => {
    const list = await fetchAllWallpapers();
    // activeId === 0 时 wallpapers 就是当前视图，同步更新避免两份数据不一致
    set((state) =>
      state.activeId === 0 ? { wallpapers: list, viewWallpapers: list } : { wallpapers: list },
    );
  },

  setActiveId: async (id: number) => {
    if (get().activeId === id) return;
    set({ activeId: id });
    await get().refreshView();
  },

  sync: async () => {
    const activeId = get().activeId;
    // 负值视图（回收站 / 显示器设置）下 refreshView 直接返回，但全局基础缓存
    // 仍要对齐：恢复 / 清空都发生在回收站视图，picker 依赖这份列表。
    // activeId === 0 时 fetchWallpapers 会同时写入 viewWallpapers，一次查询足矣。
    if (activeId <= 0) {
      await get().fetchWallpapers();
      return;
    }
    // 收藏夹视图：基础缓存与当前视图是两次不同查询，都要重取
    await Promise.all([get().fetchWallpapers(), get().refreshView()]);
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
    } finally {
      // 同样受守卫约束：过期请求不应关掉新请求的 loading
      if (seq === viewRequestSeq) set({ viewLoading: false });
    }
  },

  refreshByIds: async (ids: number[]) => {
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
  },

  importWallpapers: async () => {
    const extensions = await get().fetchSupportedExtensions();
    if (extensions.length === 0) return EMPTY_IMPORT_RESULT;

    const selected = await open({
      multiple: true,
      filters: [
        {
          name: i18n.t("main.wallpaperFiles"),
          extensions,
        },
      ],
    });

    if (!selected || selected.length === 0) {
      return { ...EMPTY_IMPORT_RESULT, cancelled: true };
    }

    set({ loading: true });
    try {
      const paths = selected as string[];
      const imported = await importWallpaperFiles(paths);

      // 新壁纸是否命中当前智能夹由后端实时求值，视图与基础缓存
      // 统一由 invoke 的失效信号驱动 sync() 重取，这里不再自行刷新

      // 异步分批生成视频缩略图（不阻塞 loading 状态）
      generateVideoThumbnails(imported, get().refreshByIds).catch((e) =>
        console.error("[VideoThumbnail] batch failed", e),
      );

      // 选择器已按扩展名过滤，正常不会出现 skipped；保留字段以统一形状
      return { imported: imported.length, skipped: 0, rejectedBySize: 0, cancelled: false };
    } finally {
      set({ loading: false });
    }
  },

  /**
   * 按路径导入（Tauri 原生拖放），与点击导入共用后端 import_wallpapers。
   *
   * 传路径而非字节，因此不需要体积守卫：字节方式必须先把整个文件读进内存，
   * 故 200MB/500MB 上限是防止前端 OOM 的保护；路径方式由后端
   * `prepare_wallpaper_files` 用 `std::fs::copy` 做文件级拷贝，进程内存占用与
   * 文件大小无关。数量也不设限——后端 `import_batch` 已用 buffer_unordered
   * 按 CPU 核数限并发。
   *
   * 前端仅按扩展名预筛，目的是给出「N 个文件格式不支持」的准确计数；
   * 真正的类型校验仍在后端 detect_file_type。
   */
  importByPaths: async (paths: string[]) => {
    const extensions = await get().fetchSupportedExtensions();
    const extensionSet = new Set(extensions);

    const matched: string[] = [];
    let skipped = 0;
    for (const p of paths) {
      const ext = p.split(".").pop()?.toLowerCase() ?? "";
      if (extensionSet.has(ext)) matched.push(p);
      else skipped += 1;
    }

    if (matched.length === 0) {
      return { imported: 0, skipped, rejectedBySize: 0, cancelled: false };
    }

    try {
      set({ loading: true });
      const imported = await importWallpaperFiles(matched);

      generateVideoThumbnails(imported, get().refreshByIds).catch((e) =>
        console.error("[VideoThumbnail] batch failed", e),
      );

      return { imported: imported.length, skipped, rejectedBySize: 0, cancelled: false };
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
      return { imported: 0, skipped, rejectedBySize: 0, cancelled: false };
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
      return { imported: 0, skipped, rejectedBySize, cancelled: false };
    }

    try {
      set({ loading: true });

      // 3) 逐文件串行导入：File → ArrayBuffer → Uint8Array，raw body 直传
      //    串行可将内存峰值控制在单个文件大小，且规避 SQLite 写锁竞争
      const imported: Wallpaper[] = [];
      const failures: string[] = [];
      for (const f of sizeAccepted) {
        try {
          const buf = await f.arrayBuffer();
          // WebView2 在生产构建（tauri:// 自定义协议）下，拖入的 File 可能
          // size 正常但读不出字节。此时必须显式失败，不能把空数据发给后端。
          if (buf.byteLength === 0) {
            throw new Error(`读取到 0 字节（file.size=${f.size}）`);
          }
          const wp = await importWallpaperBytes(f.name, new Uint8Array(buf));
          imported.push(wp);
        } catch (e) {
          console.error(`[DragImport] failed for ${f.name}`, e);
          failures.push(`${f.name}: ${e instanceof Error ? e.message : String(e)}`);
        }
      }

      // 全部失败时必须上抛：否则三个计数都是 0，上层不弹任何 toast，
      // 用户只看到 loading 一闪而过，无法区分「成功」与「静默失败」
      if (imported.length === 0 && failures.length > 0) {
        throw new Error(failures.join("; "));
      }

      // raw body 直传绕过 invoke 封装，由调用方在整批完成后统一发布一次信号；
      // 逐条发布会让 N 个文件触发 N 次 sync()（每次两次重取）。
      publishCommandSignals(COMMANDS.IMPORT_WALLPAPER_BYTES);

      // 4) 异步分批生成视频缩略图
      generateVideoThumbnails(imported, get().refreshByIds).catch((e) =>
        console.error("[VideoThumbnail] batch failed", e),
      );

      return { imported: imported.length, skipped, rejectedBySize, cancelled: false };
    } finally {
      set({ loading: false });
    }
  },

  deleteWallpapers: async (ids: number[]) => {
    const count = await deleteWallpaperBatch(ids);
    console.log(`[Trash] ${count} wallpapers moved to trash`);
  },
}));
