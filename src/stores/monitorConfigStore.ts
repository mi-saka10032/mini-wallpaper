import { create } from "zustand";
import { listen, EVENTS } from "@/api/event";
import type { MonitorConfig } from "@/api/config";
import { getMonitorConfigs, upsertMonitorConfig, deleteMonitorConfig } from "@/api/monitorConfig";
import { invoke } from "@/api/invoke";
import { COMMANDS } from "@/api/config";

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
  upsertAll: (
    params: Omit<Parameters<MonitorConfigState["upsert"]>[0], "monitorId">,
  ) => Promise<void>;
  remove: (id: number, monitorId?: string) => Promise<void>;
}

let _eventUnlisten: (() => void) | null = null;
let _configRefreshUnlisten: (() => void) | null = null;
let _initialized = false;

// HMR 安全：开发环境下模块热替换时清理事件监听器
if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    _eventUnlisten?.();
    _eventUnlisten = null;
    _configRefreshUnlisten?.();
    _configRefreshUnlisten = null;
    _initialized = false;
  });
}

export const useMonitorConfigStore = create<MonitorConfigState>((set) => ({
  configs: [],
  loading: false,

  init: async () => {
    if (_initialized) return;
    _initialized = true;

    set({ loading: true });
    try {
      // 架构重整 Phase 3：显示器检测与壁纸窗口创建已由 Rust 侧
      // UnifiedWallpaperManager 在 setup 阶段完成，前端只读取配置用于展示。
      const configs = await getMonitorConfigs();
      set({ configs });

      // 启动所有满足条件的轮播定时器
      try {
        await invoke(COMMANDS.START_TIMERS);
      } catch (e) {
        console.error("[monitorConfigStore.init] start_timers failed:", e);
      }

      // 监听 thumbnail-changed 事件（主窗口缩略图更新）
      if (!_eventUnlisten) {
        _eventUnlisten = await listen(EVENTS.THUMBNAIL_CHANGED, ({ monitor_id, wallpaper_id }) => {
          set((state) => ({
            configs: state.configs.map((c) =>
              c.monitor_id === monitor_id ? { ...c, wallpaper_id } : c,
            ),
          }));
        });
      }

      // 监听 monitor-config-refreshed 事件（后端删除操作导致 config 变更，重新拉取）
      if (!_configRefreshUnlisten) {
        _configRefreshUnlisten = await listen(EVENTS.MONITOR_CONFIG_REFRESHED, async () => {
          const configs = await getMonitorConfigs();
          set({ configs });
        });
      }
    } finally {
      set({ loading: false });
    }
  },

  syncMonitors: async () => {
    // 前端热插拔功能已取消：显示器参数只读，变更由 Rust 侧事件驱动。
    // 保留空实现以兼容既有调用点，后续 M20 会彻底移除该入口。
    await useMonitorConfigStore.getState().fetchConfigs();
  },

  fetchConfigs: async () => {
    set({ loading: true });
    try {
      const configs = await getMonitorConfigs();
      set({ configs });
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

    // 壁纸窗口副作用由后端 upsert command 内的 reconcile 完成，前端不再编排。
    return config;
  },

  syncConfigToAll: async (sourceMonitorId) => {
    const state = useMonitorConfigStore.getState();
    const source = state.configs.find((c) => c.monitor_id === sourceMonitorId && c.active);
    if (!source) return;

    const others = state.configs.filter((c) => c.active && c.monitor_id !== sourceMonitorId);
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

    // 壁纸窗口副作用由后端逐条 upsert 触发的 reconcile 完成，前端不再编排。
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

    // 壁纸窗口副作用由后端逐条 upsert 触发的 reconcile 完成，前端不再编排。
  },

  remove: async (id, _monitorId) => {
    await deleteMonitorConfig(id);
    set((state) => ({
      configs: state.configs.filter((c) => c.id !== id),
    }));
  },
}));
