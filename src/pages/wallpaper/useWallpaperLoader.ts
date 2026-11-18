import { useCallback, useEffect, useState } from "react";
import { listen, EVENTS } from "@/api/event";
import type { Wallpaper, MonitorConfig } from "@/api/config";
import { getMonitorConfig, getSetting, getWallpaperById } from "./api";

export function useWallpaperLoader(monitorId: string | null) {
  const [wallpaper, setWallpaper] = useState<Wallpaper | null>(null);
  const [fitMode, setFitMode] = useState<string>("cover");
  const [displayMode, setDisplayMode] = useState<string>("independent");
  const [volume, setVolume] = useState<number>(0);

  const loadFromConfig = useCallback(async (config: MonitorConfig) => {
    setFitMode(config.fit_mode || "cover");

    if (!config.wallpaper_id) {
      setWallpaper(null);
      return;
    }

    try {
      const detail = await getWallpaperById(config.wallpaper_id);
      setWallpaper(detail);
    } catch (e) {
      console.error("[useWallpaperLoader] getById failed:", e);
    }
  }, []);

  const loadConfig = useCallback(
    async (monitorId: string) => {
      try {
        const config = await getMonitorConfig(monitorId);
        if (config) loadFromConfig(config);
      } catch (e) {
        console.error("[useWallpaperLoader] loadConfig failed:", e);
      }
    },
    [loadFromConfig],
  );

  useEffect(() => {
    if (!monitorId) return;

    loadConfig(monitorId);

    getSetting("display_mode")
      .then((val) => {
        if (val) setDisplayMode(val);
      })
      .catch(() => {});

    getSetting("global_volume")
      .then((val) => {
        const v = Number(val ?? "0");
        setVolume(Math.min(Math.max(v, 0), 100));
      })
      .catch(() => {});
  }, [monitorId, loadConfig]);

  useEffect(() => {
    if (!monitorId) return;

    const unlisteners: Promise<() => void>[] = [];

    unlisteners.push(
      listen(EVENTS.WALLPAPER_CHANGED, (payload) => {
        if (payload.monitor_id === monitorId) {
          loadConfig(monitorId);
        }
      }),
    );

    unlisteners.push(
      listen(EVENTS.WALLPAPER_CLEARED, (payload) => {
        if (payload.monitor_id === monitorId) {
          setWallpaper(null);
        }
      }),
    );

    unlisteners.push(
      listen(EVENTS.FIT_MODE_CHANGED, (payload) => {
        if (payload.monitor_id === monitorId) {
          setFitMode(payload.fit_mode);
        }
      }),
    );

    unlisteners.push(
      listen(EVENTS.DISPLAY_MODE_CHANGED, (payload) => {
        if (payload.monitor_id === monitorId) {
          setDisplayMode(payload.display_mode);
          loadConfig(monitorId);
        }
      }),
    );

    unlisteners.push(
      listen(EVENTS.VOLUME_CHANGED, (payload) => {
        setVolume(Math.min(Math.max(payload.volume, 0), 100));
      }),
    );

    return () => {
      for (const p of unlisteners) {
        p.then((fn) => fn());
      }
    };
  }, [monitorId, loadConfig]);

  return { wallpaper, fitMode, displayMode, volume };
}
