import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMonitorConfigStore } from "@/stores/monitorConfigStore";
import { getCollectionWallpapers } from "@/api/collection";

export interface UseMonitorConfigOptions {
  selectedMonitorId: string | null;
  isSyncMode: boolean;
}

export function useMonitorConfig({ selectedMonitorId, isSyncMode }: UseMonitorConfigOptions) {
  const { t } = useTranslation();
  const upsert = useMonitorConfigStore((s) => s.upsert);
  const upsertAll = useMonitorConfigStore((s) => s.upsertAll);

  const [collectionWarning, setCollectionWarning] = useState<string | null>(null);
  const [sourceType, setSourceType] = useState<"wallpaper" | "collection">("wallpaper");

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

      if (source === "wallpaper") {
        await upsertCurrent({ clearCollection: true });
      }
    },
    [upsertCurrent, selectedMonitorId],
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

  /** 同步 sourceType 到外部（当选中配置变化时调用） */
  const syncSourceType = useCallback((hasCollection: boolean) => {
    setSourceType(hasCollection ? "collection" : "wallpaper");
  }, []);

  return {
    collectionWarning,
    sourceType,
    syncSourceType,
    handleSourceChange,
    handleWallpaperSelect,
    handleCollectionSelect,
    handleFitModeChange,
    handlePlayModeChange,
    handleIntervalChange,
    handleEnabledChange,
  };
}

/** 间隔预设值（秒） */
export const INTERVAL_PRESETS = [10, 30, 60, 300, 600, 1800, 3600, 7200];

/** 格式化间隔显示 */
export function formatInterval(seconds: number, t: (key: string, opts?: Record<string, unknown>) => string): string {
  if (seconds < 60) return t("time.seconds", { count: seconds });
  if (seconds < 3600) return t("time.minutes", { count: Math.round(seconds / 60) });
  return t("time.hours", { count: Math.round(seconds / 3600) });
}
