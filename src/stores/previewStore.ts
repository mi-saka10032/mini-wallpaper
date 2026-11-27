import { create } from "zustand";

import type { Wallpaper } from "@/api/config";

/**
 * 预览播放列表的单一数据源。
 *
 * 为什么放在 store 而不是 App 的局部 state：灯箱拿到的列表必须等于网格
 * 「实际展示的那批」（含关键词过滤 / 排序 / 管理模式派生），而这份列表在
 * MainContent 内部计算。取消收藏、改标签等写操作经 pubsub 刷新
 * viewWallpapers 后，MainContent 会把新派生列表同步回这里，灯箱据此做
 * 「锚点 id 消失」的重定位——不在点击回调里预判。
 */
interface PreviewStore {
  wallpapers: Wallpaper[];
  /** 打开时的初始索引；null 表示未打开 */
  index: number | null;
  open: (wallpapers: Wallpaper[], index: number) => void;
  close: () => void;
  /** 网格派生列表变化时同步，仅在预览打开期间生效 */
  syncList: (wallpapers: Wallpaper[]) => void;
}

export const usePreviewStore = create<PreviewStore>((set, get) => ({
  wallpapers: [],
  index: null,

  open: (wallpapers, index) => set({ wallpapers, index }),

  close: () => set({ index: null, wallpapers: [] }),

  syncList: (wallpapers) => {
    if (get().index === null) return;
    set({ wallpapers });
  },
}));
