import { create } from "zustand";
import { getSettings, setSetting } from "@/api/appSetting";

/** 设置 key 常量 */
export const SETTING_KEYS = {
  THEME: "theme",
  ACCENT_COLOR: "accent_color",
  LANGUAGE: "language",
  CLOSE_TO_TRAY: "close_to_tray",
  PAUSE_ON_FULLSCREEN: "pause_on_fullscreen",
  GLOBAL_VOLUME: "global_volume",
  DISPLAY_MODE: "display_mode",
  SHORTCUT_NEXT: "shortcut_next_wallpaper",
  SHORTCUT_PREV: "shortcut_prev_wallpaper",
} as const;

interface SettingState {
  /** 所有设置的 key-value 映射 */
  settings: Record<string, string>;
  loading: boolean;

  /** 从 DB 加载所有设置 */
  fetchSettings: () => Promise<void>;

  /** 更新单个设置（DB + store 同步） */
  updateSetting: (key: string, value: string, monitorId?: string) => Promise<void>;

  /** 便捷 getter：获取某个 key 的值（带默认值） */
  get: (key: string, defaultValue?: string) => string;
}

export const useSettingStore = create<SettingState>((set, get) => ({
  settings: {},
  loading: false,

  fetchSettings: async () => {
    try {
      set({ loading: true });
      const settings = await getSettings();
      set({ settings });
    } catch (e) {
      console.error("[fetchSettings]", e);
    } finally {
      set({ loading: false });
    }
  },

  updateSetting: async (key: string, value: string, monitorId?: string) => {
    try {
      await setSetting(key, value, monitorId);
      set((state) => ({
        settings: { ...state.settings, [key]: value },
      }));
    } catch (e) {
      console.error("[updateSetting]", e);
    }
  },

  get: (key: string, defaultValue = "") => {
    return get().settings[key] ?? defaultValue;
  },
}));

/**
 * 细粒度 selector hook：只订阅单个 setting key 的值变化
 * 使用自定义 equality 比较，避免 settings 对象引用变化导致无关组件重渲染
 */
export function useSetting(key: string, defaultValue = ""): string {
  return useSettingStore((s) => s.settings[key] ?? defaultValue);
}

/**
 * 获取 updateSetting action（引用稳定，不会触发重渲染）
 */
export function useUpdateSetting() {
  return useSettingStore((s) => s.updateSetting);
}