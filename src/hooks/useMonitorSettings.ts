import { useCallback, useEffect, useMemo, useState } from "react";
import { availableMonitors } from "@tauri-apps/api/window";
import { useMonitorConfigStore } from "@/stores/monitorConfigStore";
import { useSettingStore, SETTING_KEYS } from "@/stores/settingStore";
import { useCollectionStore } from "@/stores/collectionStore";
import { useWallpaperStore } from "@/stores/wallpaperStore";
import type { Wallpaper } from "@/api/config";
import { INTERVAL_PRESETS } from "@/hooks/useMonitorConfig";
import { syncWallpaperWindows } from "@/stores/monitorSync";

export function useMonitorSettings() {
  // Store
  const configs = useMonitorConfigStore((s) => s.configs);
  const syncMonitors = useMonitorConfigStore((s) => s.syncMonitors);
  const loading = useMonitorConfigStore((s) => s.loading);
  const collections = useCollectionStore((s) => s.collections);
  const fetchCollections = useCollectionStore((s) => s.fetchCollections);
  const wallpapers = useWallpaperStore((s) => s.wallpapers);

  // 全局 display_mode（从 app_setting 读取）
  const displayMode = useSettingStore((s) => s.settings[SETTING_KEYS.DISPLAY_MODE] ?? "independent");
  const updateSetting = useSettingStore((s) => s.updateSetting);

  // 只使用 active 的 config 作为显示器列表
  const activeConfigs = useMemo(
    () => configs.filter((c) => c.active),
    [configs],
  );

  // 选中的 monitor index（内部 raw 状态）
  const [rawSelectedIndex, setSelectedIndex] = useState<number>(0);

  // 初始化获取收藏夹列表
  useEffect(() => {
    fetchCollections();
  }, [fetchCollections]);

  // 安全的 selectedIndex：通过 useMemo 派生，避免越界
  const selectedIndex = useMemo(
    () => (activeConfigs.length > 0 && rawSelectedIndex >= activeConfigs.length) ? 0 : rawSelectedIndex,
    [rawSelectedIndex, activeConfigs.length],
  );

  const selectedConfig = activeConfigs[selectedIndex] ?? null;
  const selectedMonitorId = selectedConfig?.monitor_id ?? null;

  /**
   * 是否处于同步模式（mirror / extend）
   * 在此模式下，所有设置修改都同步到全部 active 显示器
   */
  const isSyncMode = displayMode === "mirror" || displayMode === "extend";

  // 获取壁纸缩略图（从 store 缓存中查找）
  const getWallpaperThumb = useCallback(
    (wallpaperId: number | null): Wallpaper | null => {
      if (!wallpaperId) return null;
      return wallpapers.find((w) => w.id === wallpaperId) ?? null;
    },
    [wallpapers],
  );

  const handleDisplayModeChange = useCallback(
    async (newDisplayMode: string) => {
      if (!selectedMonitorId) return;

      await updateSetting(SETTING_KEYS.DISPLAY_MODE, newDisplayMode, selectedMonitorId);

      if (newDisplayMode === "mirror" || newDisplayMode === "extend") {
        const fetchConfigs = useMonitorConfigStore.getState().fetchConfigs;
        await fetchConfigs();

        // 后端已将壁纸配置同步到所有显示器，需为尚未创建窗口的显示器创建壁纸窗口
        const configs = useMonitorConfigStore.getState().configs;
        const monitors = await availableMonitors();
        await syncWallpaperWindows(configs, monitors);
      }
    },
    [updateSetting, selectedMonitorId],
  );

  // 间隔滑块值
  const intervalSliderValue = useMemo(() => {
    const interval = selectedConfig?.play_interval ?? 300;
    const idx = INTERVAL_PRESETS.findIndex((v) => v >= interval);
    return idx >= 0 ? idx : 4;
  }, [selectedConfig]);

  const handleRefresh = useCallback(() => {
    syncMonitors();
  }, [syncMonitors]);

  return {
    loading,
    activeConfigs,
    selectedIndex,
    selectedConfig,
    selectedMonitorId,
    displayMode,
    isSyncMode,
    collections,
    wallpapers,
    intervalSliderValue,
    setSelectedIndex,
    getWallpaperThumb,
    handleDisplayModeChange,
    handleRefresh,
  };
}