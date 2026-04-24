import { create } from "zustand";
import { availableMonitors, type Monitor } from "@tauri-apps/api/window";
import { listen, EVENTS } from "@/api/event";
import type { MonitorConfig } from "@/api/config";
import {
  getMonitorConfigs,
  upsertMonitorConfig,
  deleteMonitorConfig,
} from "@/api/monitorConfig";
import {
  createWallpaperWindow,
  destroyWallpaperWindow,
} from "@/api/wallpaperWindow";

interface MonitorConfigState {
  configs: MonitorConfig[];
  loading: boolean;
  /** 初始化：检测显示器 → 批量 upsert active → 创建壁纸窗口 → 监听事件 */
  init: () => Promise<void>;
  /** 同步显示器状态（热插拔时调用，可重复执行） */
  syncMonitors: () => Promise<void>;
  fetchConfigs: () => Promise<void>;
  upsert: (params: {
    monitorId: string;
    wallpaperId?: number | null;
    collectionId?: number | null;
    clearCollection?: boolean;
    displayMode?: string;
    fitMode?: string;
    playMode?: string;
    playInterval?: number;
    isEnabled?: boolean;
    active?: boolean;
  }) => Promise<MonitorConfig>;
  remove: (id: number, monitorId?: string) => Promise<void>;
}

let _eventUnlisten: (() => void) | null = null;
let _initialized = false;

/** 已创建壁纸窗口的 monitor_id 集合（前端侧跟踪，防止重复创建） */
const _activeWallpaperWindows = new Set<string>();

/**
 * 为 active 且有 wallpaper_id 的显示器创建壁纸窗口
 * 支持三种 display_mode:
 * - independent: 每个显示器独立壁纸
 * - mirror: 所有显示器使用同一壁纸（数据由 Rust 保证一致，窗口正常创建）
 * - extend: 一张壁纸横跨所有显示器，每个窗口渲染对应区域
 */
async function syncWallpaperWindows(
  configs: MonitorConfig[],
  monitors: Monitor[],
) {
  const monitorMap = new Map(
    monitors.map((m) => [m.name ?? `monitor_${monitors.indexOf(m)}`, m]),
  );

  // extend 模式需要计算所有显示器总宽度
  const extendConfigs = configs.filter(
    (c) => c.active && c.wallpaper_id && c.display_mode === "extend",
  );
  let extendTotalWidth = 0;
  const extendOffsets = new Map<string, number>();
  if (extendConfigs.length > 0) {
    // 按 x 位置排序，计算每个显示器在总宽度中的偏移
    const sorted = extendConfigs
      .map((c) => ({ config: c, monitor: monitorMap.get(c.monitor_id) }))
      .filter((item) => item.monitor != null)
      .sort((a, b) => a.monitor!.position.x - b.monitor!.position.x);

    for (const item of sorted) {
      extendOffsets.set(item.config.monitor_id, extendTotalWidth);
      extendTotalWidth += item.monitor!.size.width;
    }
  }

  const activeIds = new Set<string>();

  for (const config of configs) {
    if (!config.active || !config.wallpaper_id) continue;

    const monitor = monitorMap.get(config.monitor_id);
    if (!monitor) continue;

    activeIds.add(config.monitor_id);

    // 已创建则跳过
    if (_activeWallpaperWindows.has(config.monitor_id)) continue;

    try {
      const pos = monitor.position;
      const size = monitor.size;

      // extend 模式：附带视口偏移参数
      let extraQuery: string | undefined;
      if (config.display_mode === "extend" && extendTotalWidth > 0) {
        const offsetX = extendOffsets.get(config.monitor_id) ?? 0;
        extraQuery = `extendOffsetX=${offsetX}&extendTotalWidth=${extendTotalWidth}&extendMyWidth=${size.width}`;
      }

      await createWallpaperWindow(
        config.monitor_id,
        pos.x,
        pos.y,
        size.width,
        size.height,
        extraQuery,
      );
      _activeWallpaperWindows.add(config.monitor_id);
      console.log(`[syncWallpaperWindows] Created window for ${config.monitor_id} (${config.display_mode})`);
    } catch (e) {
      console.error(`[syncWallpaperWindows] Failed to create window for ${config.monitor_id}:`, e);
    }
  }

  // 销毁不再 active 的壁纸窗口
  for (const monitorId of _activeWallpaperWindows) {
    if (!activeIds.has(monitorId)) {
      try {
        await destroyWallpaperWindow(monitorId);
        _activeWallpaperWindows.delete(monitorId);
        console.log(`[syncWallpaperWindows] Destroyed window for ${monitorId}`);
      } catch (e) {
        console.error(`[syncWallpaperWindows] Failed to destroy window for ${monitorId}:`, e);
      }
    }
  }
}

/** 核心同步逻辑：检测物理显示器 → 比对 DB → upsert → 同步壁纸窗口 → 刷新 store */
async function doSyncMonitors(set: (partial: Partial<MonitorConfigState>) => void) {
  // 1. 检测当前物理显示器
  const monitors = await availableMonitors();
  const activeMonitorIds = new Set(
    monitors.map((m) => m.name ?? `monitor_${monitors.indexOf(m)}`),
  );

  // 2. 获取现有 configs
  const existingConfigs = await getMonitorConfigs();
  const existingIds = new Set(existingConfigs.map((c) => c.monitor_id));

  // 3. 批量 upsert：激活匹配的、关闭不匹配的、新增不存在的
  const upsertPromises: Promise<MonitorConfig>[] = [];

  for (const config of existingConfigs) {
    const shouldBeActive = activeMonitorIds.has(config.monitor_id);
    if (config.active !== shouldBeActive) {
      upsertPromises.push(
        upsertMonitorConfig({
          monitorId: config.monitor_id,
          active: shouldBeActive,
        }),
      );
    }
  }

  for (const mid of activeMonitorIds) {
    if (!existingIds.has(mid)) {
      upsertPromises.push(
        upsertMonitorConfig({
          monitorId: mid,
          active: true,
        }),
      );
    }
  }

  if (upsertPromises.length > 0) {
    await Promise.all(upsertPromises);
  }

  // 4. 重新获取最新 configs
  const configs = await getMonitorConfigs();
  set({ configs });

  // 5. 同步壁纸窗口（创建/销毁）
  await syncWallpaperWindows(configs, monitors);
}

export const useMonitorConfigStore = create<MonitorConfigState>((set) => ({
  configs: [],
  loading: false,

  init: async () => {
    if (_initialized) return;
    _initialized = true;

    try {
      set({ loading: true });

      await doSyncMonitors(set);

      // 监听 thumbnail-changed 事件（主窗口缩略图更新）
      if (!_eventUnlisten) {
        _eventUnlisten = await listen(
          EVENTS.THUMBNAIL_CHANGED,
          ({ monitor_id, wallpaper_id }) => {
            set((state) => ({
              configs: state.configs.map((c) =>
                c.monitor_id === monitor_id
                  ? { ...c, wallpaper_id }
                  : c,
              ),
            }));
          },
        );
      }
    } catch (e) {
      console.error("[monitorConfigStore.init]", e);
    } finally {
      set({ loading: false });
    }
  },

  syncMonitors: async () => {
    try {
      set({ loading: true });
      await doSyncMonitors(set);
    } catch (e) {
      console.error("[monitorConfigStore.syncMonitors]", e);
    } finally {
      set({ loading: false });
    }
  },

  fetchConfigs: async () => {
    set({ loading: true });
    try {
      const configs = await getMonitorConfigs();
      set({ configs });
    } catch (e) {
      console.error("[fetchMonitorConfigs]", e);
    } finally {
      set({ loading: false });
    }
  },

  upsert: async (params) => {
    const config = await upsertMonitorConfig(params);
    set((state) => {
      const idx = state.configs.findIndex((c) => c.monitor_id === config.monitor_id);
      if (idx >= 0) {
        const updated = [...state.configs];
        updated[idx] = config;
        return { configs: updated };
      }
      return { configs: [...state.configs, config] };
    });
    return config;
  },

  remove: async (id, _monitorId) => {
    await deleteMonitorConfig(id);
    set((state) => ({
      configs: state.configs.filter((c) => c.id !== id),
    }));
  },
}));
