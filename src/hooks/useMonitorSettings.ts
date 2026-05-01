import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMonitorConfigStore } from "@/stores/monitorConfigStore";
import { useSettingStore, SETTING_KEYS } from "@/stores/settingStore";
import { useCollectionStore } from "@/stores/collectionStore";
import { useWallpaperStore } from "@/stores/wallpaperStore";
import { getWallpapers as getCollectionWallpapers } from "@/api/collection";
import type { Wallpaper } from "@/api/config";

/** 间隔预设值（秒） */
export const INTERVAL_PRESETS = [10, 30, 60, 300, 600, 1800, 3600, 7200];

/** 格式化间隔显示 */
export function formatInterval(seconds: number, t: (key: string, opts?: Record<string, unknown>) => string): string {
  if (seconds < 60) return t("time.seconds", { count: seconds });
  if (seconds < 3600) return t("time.minutes", { count: Math.round(seconds / 60) });
  return t("time.hours", { count: Math.round(seconds / 3600) });
}

export function useMonitorSettings() {
  const { t } = useTranslation();

  // Store
  const configs = useMonitorConfigStore((s) => s.configs);
  const syncMonitors = useMonitorConfigStore((s) => s.syncMonitors);
  const upsert = useMonitorConfigStore((s) => s.upsert);
  const upsertAll = useMonitorConfigStore((s) => s.upsertAll);
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

  // 选中的 monitor index
  const [selectedIndex, setSelectedIndex] = useState<number>(0);

  useEffect(() => {
    fetchCollections();
  }, [fetchCollections]);

  // 选中范围保护
  useEffect(() => {
    if (activeConfigs.length > 0 && selectedIndex >= activeConfigs.length) {
      setSelectedIndex(0);
    }
  }, [activeConfigs.length, selectedIndex]);

  const selectedConfig = activeConfigs[selectedIndex] ?? null;
  const selectedMonitorId = selectedConfig?.monitor_id ?? null;

  /**
   * 是否处于同步模式（mirror / extend）
   * 在此模式下，所有设置修改都同步到全部 active 显示器
   */
  const isSyncMode = displayMode === "mirror" || displayMode === "extend";

  // 获取壁纸缩略图
  const getWallpaperThumb = useCallback(
    (wallpaperId: number | null): Wallpaper | null => {
      if (!wallpaperId) return null;
      return wallpapers.find((w) => w.id === wallpaperId) ?? null;
    },
    [wallpapers],
  );

  // ===== 配置更新 =====
  const [collectionWarning, setCollectionWarning] = useState<string | null>(null);
  const [sourceType, setSourceType] = useState<"wallpaper" | "collection">("wallpaper");

  // 当选中显示器或配置变化时同步 sourceType
  useEffect(() => {
    if (selectedConfig?.collection_id) {
      setSourceType("collection");
    } else {
      setSourceType("wallpaper");
    }
  }, [selectedConfig?.collection_id, selectedMonitorId]);

  // ===== 通用 upsert 封装：同步模式下自动广播到所有显示器 =====
  const upsertCurrent = useCallback(
    async (params: Omit<Parameters<typeof upsert>[0], "monitorId">) => {
      if (!selectedMonitorId) return;
      if (isSyncMode) {
        await upsertAll(params);
      } else {
        await upsert({ ...params, monitorId: selectedMonitorId });
      }
    },
    [upsert, upsertAll, selectedMonitorId, isSyncMode],
  );

  const handleSourceChange = useCallback(
    async (source: "wallpaper" | "collection") => {
      if (!selectedMonitorId) return;
      setCollectionWarning(null);
      setSourceType(source);

      if (source === "wallpaper" && selectedConfig?.collection_id) {
        await upsertCurrent({ clearCollection: true });
      }
    },
    [upsertCurrent, selectedMonitorId, selectedConfig?.collection_id],
  );

  const handleWallpaperSelect = useCallback(
    async (wallpaperId: number) => {
      if (!selectedMonitorId) return;
      await upsertCurrent({ wallpaperId });
    },
    [upsertCurrent, selectedMonitorId],
  );

  const handleCollectionSelect = useCallback(
    async (collectionIdStr: string) => {
      if (!selectedMonitorId) return;
      const collectionId = Number(collectionIdStr);
      setCollectionWarning(null);

      try {
        const wallpapersInCollection = await getCollectionWallpapers(collectionId);

        if (wallpapersInCollection.length === 0) {
          setCollectionWarning(t("monitor.collectionEmptyWarn"));
          return;
        }

        const firstWallpaperId = wallpapersInCollection[0].id;
        await upsertCurrent({
          collectionId,
          wallpaperId: firstWallpaperId,
        });
      } catch (e) {
        console.error("[handleCollectionSelect]", e);
        setCollectionWarning(t("monitor.collectionQueryFail"));
      }
    },
    [upsertCurrent, selectedMonitorId, t],
  );

  const handleFitModeChange = useCallback(
    async (fitMode: string) => {
      await upsertCurrent({ fitMode });
    },
    [upsertCurrent],
  );

  const handleDisplayModeChange = useCallback(
    async (newDisplayMode: string) => {
      if (!selectedMonitorId) return;

      await updateSetting(SETTING_KEYS.DISPLAY_MODE, newDisplayMode, selectedMonitorId);

      if (newDisplayMode === "mirror" || newDisplayMode === "extend") {
        const fetchConfigs = useMonitorConfigStore.getState().fetchConfigs;
        await fetchConfigs();
      }
    },
    [updateSetting, selectedMonitorId],
  );

  const handlePlayModeChange = useCallback(
    async (playMode: string) => {
      await upsertCurrent({ playMode });
    },
    [upsertCurrent],
  );

  const handleIntervalChange = useCallback(
    async (value: number[]) => {
      const seconds = INTERVAL_PRESETS[value[0]] ?? 300;
      await upsertCurrent({ playInterval: seconds });
    },
    [upsertCurrent],
  );

  const handleEnabledChange = useCallback(
    async (enabled: boolean) => {
      await upsertCurrent({ isEnabled: enabled });
    },
    [upsertCurrent],
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
    // 状态
    loading,
    activeConfigs,
    selectedIndex,
    selectedConfig,
    selectedMonitorId,
    displayMode,
    isSyncMode,
    collections,
    wallpapers,
    collectionWarning,
    sourceType,
    intervalSliderValue,

    // 操作
    setSelectedIndex,
    getWallpaperThumb,
    handleSourceChange,
    handleWallpaperSelect,
    handleCollectionSelect,
    handleFitModeChange,
    handleDisplayModeChange,
    handlePlayModeChange,
    handleIntervalChange,
    handleEnabledChange,
    handleRefresh,
  };
}
