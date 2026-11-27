import { create } from "zustand";

import type { ApplySceneResult, SceneState, SceneSummary } from "@/api/config";
import * as sceneApi from "@/api/scene";

interface SceneStoreState {
  /** 当前桌面状态，与即将保存的 payload 同源 */
  current: SceneState | null;
  scenes: SceneSummary[];
  loading: boolean;
  /** 弹窗打开时拉取一次；保存 / 应用成功后再次调用 */
  refresh: () => Promise<void>;
  save: (name: string) => Promise<SceneSummary>;
  rename: (id: number, name: string) => Promise<void>;
  remove: (id: number) => Promise<void>;
  apply: (id: number) => Promise<ApplySceneResult>;
}

export const useSceneStore = create<SceneStoreState>((set, get) => ({
  current: null,
  scenes: [],
  loading: false,

  refresh: async () => {
    set({ loading: true });
    try {
      const [current, scenes] = await Promise.all([
        sceneApi.getCurrentSceneState(),
        sceneApi.listScenes(),
      ]);
      set({ current, scenes });
    } finally {
      set({ loading: false });
    }
  },

  save: async (name) => {
    const saved = await sceneApi.saveScene(name);
    await get().refresh();
    return saved;
  },

  rename: async (id, name) => {
    await sceneApi.renameScene(id, name);
    await get().refresh();
  },

  remove: async (id) => {
    await sceneApi.deleteScene(id);
    await get().refresh();
  },

  apply: async (id) => {
    const result = await sceneApi.applyScene(id);
    // 应用会改 monitor_configs 与 app_settings，重新拉一次当前状态与列表。
    await get().refresh();
    return result;
  },
}));
