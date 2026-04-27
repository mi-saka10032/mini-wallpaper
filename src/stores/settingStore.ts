import { create } from "zustand";
import { getSettings, setSetting } from "@/api/appSetting";

/** 设置 key 常量 */
export const SETTING_KEYS = {
  THEME: "theme",
  LANGUAGE: "language",
  CLOSE_TO_TRAY: "close_to_tray",
  PAUSE_ON_FULLSCREEN: "pause_on_fullscreen",
  GLOBAL_VOLUME: "global_volume",
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
  updateSetting: (key: string, value: string) => Promise<void>;

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

  updateSetting: async (key: string, value: string) => {
    try {
      await setSetting(key, value);
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
