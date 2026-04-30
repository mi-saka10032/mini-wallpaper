import { create } from "zustand";
import { availableMonitors, type Monitor } from "@tauri-apps/api/window";
import { listen, EVENTS } from "@/api/event";
import type { MonitorConfig } from "@/api/config";
import {
  getMonitorConfigs,
  upsertMonitorConfig,
  deleteMonitorConfig,
} from "@/api/monitorConfig";
import { invoke } from "@/api/invoke";
import { COMMANDS } from "@/api/config";
import {
  createWallpaperWindow,
  destroyWallpaperWindow,
  getActiveWallpaperWindows,
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
    fitMode?: string;
    playMode?: string;
    playInterval?: number;
    isEnabled?: boolean;
    active?: boolean;
  }) => Promise<MonitorConfig>;
  /**
   * 将指定 config 的全部设置（除 id/monitor_id 外）同步到所有其他 active 显示器
   * 用于 mirror/extend 模式切换时，将当前选中显示器的配置复制到其他显示器
   */
  syncConfigToAll: (sourceMonitorId: string) => Promise<void>;
  /**
   * 批量 upsert：对所有 active 显示器执行相同的更新参数
   * 用于 mirror/extend 模式下修改设置时同步到所有显示器
   */
  upsertAll: (params: Omit<Parameters<MonitorConfigState["upsert"]>[0], "monitorId">) => Promise<void>;
  remove: (id: number, monitorId?: string) => Promise<void>;
}

let _eventUnlisten: (() => void) | null = null;
let _configRefreshUnlisten: (() => void) | null = null;
let _initialized = false;

/** 已创建壁纸窗口的 monitor_id 集合（前端侧跟踪，防止重复创建） */
const _activeWallpaperWindows = new Set<string>();

/**
 * 为 active 且有 wallpaper_id 的显示器创建壁纸窗口
 * display_mode 从全局 app_setting 读取：
 * - independent: 每个显示器独立壁纸
 * - mirror: 所有显示器使用同一壁纸（数据由 Rust 保证一致，窗口正常创建）
 * - extend: 一张壁纸横跨所有显示器，每个窗口通过 availableMonitors() 自行计算裁剪区域
 */
async function syncWallpaperWindows(
  configs: MonitorConfig[],
  monitors: Monitor[],
) {
  // 先从 Rust 端查询已有壁纸窗口，恢复前端跟踪集合（解决页面刷新后状态丢失问题）
  try {
    const existingWindows = await getActiveWallpaperWindows();
    for (const mid of existingWindows) {
      _activeWallpaperWindows.add(mid);
    }
  } catch (e) {
    console.warn("[syncWallpaperWindows] Failed to query existing windows:", e);
  }

  const monitorMap = new Map(
    monitors.map((m) => [m.name ?? `monitor_${monitors.indexOf(m)}`, m]),
  );

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

      await createWallpaperWindow(
        config.monitor_id,
        pos.x,
        pos.y,
        size.width,
        size.height,
      );
      _activeWallpaperWindows.add(config.monitor_id);
      console.log(`[syncWallpaperWindows] Created window for ${config.monitor_id}`);
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

      // 壁纸窗口创建完成后，启动所有满足条件的轮播定时器
      try {
        await invoke(COMMANDS.START_TIMERS);
      } catch (e) {
        console.error("[monitorConfigStore.init] start_timers failed:", e);
      }

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

      // 监听 monitor-config-refreshed 事件（后端删除操作导致 config 变更，重新拉取）
      if (!_configRefreshUnlisten) {
        _configRefreshUnlisten = await listen(
          EVENTS.MONITOR_CONFIG_REFRESHED,
          async () => {
            try {
              const configs = await getMonitorConfigs();
              set({ configs });
            } catch (e) {
              console.error("[monitorConfigStore] config refresh failed:", e);
            }
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

    // 设置了 wallpaperId 时，触发壁纸窗口同步（按需创建尚未存在的窗口）
    if (params.wallpaperId) {
      const monitors = await availableMonitors();
      const configs = useMonitorConfigStore.getState().configs;
      await syncWallpaperWindows(configs, monitors);
    }

    return config;
  },

  syncConfigToAll: async (sourceMonitorId) => {
    const state = useMonitorConfigStore.getState();
    const source = state.configs.find(
      (c) => c.monitor_id === sourceMonitorId && c.active,
    );
    if (!source) return;

    const others = state.configs.filter(
      (c) => c.active && c.monitor_id !== sourceMonitorId,
    );
    if (others.length === 0) return;

    const promises = others.map((c) =>
      upsertMonitorConfig({
        monitorId: c.monitor_id,
        wallpaperId: source.wallpaper_id,
        collectionId: source.collection_id,
        clearCollection: !source.collection_id,
        fitMode: source.fit_mode,
        playMode: source.play_mode,
        playInterval: source.play_interval,
        isEnabled: source.is_enabled,
      }),
    );

    const results = await Promise.all(promises);

    set((state) => {
      const updated = [...state.configs];
      for (const config of results) {
        const idx = updated.findIndex((c) => c.monitor_id === config.monitor_id);
        if (idx >= 0) {
          updated[idx] = config;
        }
      }
      return { configs: updated };
    });

    // 同步壁纸窗口（source 有 wallpaper_id 时，其他显示器也需要创建窗口）
    if (source.wallpaper_id) {
      const monitors = await availableMonitors();
      const configs = useMonitorConfigStore.getState().configs;
      await syncWallpaperWindows(configs, monitors);
    }
  },

  upsertAll: async (params) => {
    const state = useMonitorConfigStore.getState();
    const activeConfigs = state.configs.filter((c) => c.active);
    if (activeConfigs.length === 0) return;

    const promises = activeConfigs.map((c) =>
      upsertMonitorConfig({ ...params, monitorId: c.monitor_id }),
    );

    const results = await Promise.all(promises);

    set((state) => {
      const updated = [...state.configs];
      for (const config of results) {
        const idx = updated.findIndex((c) => c.monitor_id === config.monitor_id);
        if (idx >= 0) {
          updated[idx] = config;
        }
      }
      return { configs: updated };
    });

    // 设置了 wallpaperId 时，触发壁纸窗口同步（按需创建尚未存在的窗口）
    if (params.wallpaperId) {
      const monitors = await availableMonitors();
      const configs = useMonitorConfigStore.getState().configs;
      await syncWallpaperWindows(configs, monitors);
    }
  },

  remove: async (id, _monitorId) => {
    await deleteMonitorConfig(id);
    set((state) => ({
      configs: state.configs.filter((c) => c.id !== id),
    }));
  },
}));