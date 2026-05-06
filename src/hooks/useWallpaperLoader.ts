import { useCallback, useEffect, useState } from "react";
import { invoke } from "@/api/invoke";
import { listen, EVENTS } from "@/api/event";
import { COMMANDS, type Wallpaper, type MonitorConfig } from "@/api/config";
import { getById as getWallpaperById } from "@/api/wallpaper";
import { useWallpaperStore } from "@/stores/wallpaperStore";

/**
 * useWallpaperLoader - 加载壁纸数据并监听变更事件
 *
 * 职责：
 * - 初始化时根据 monitorId 读取 config → 获取壁纸
 * - 监听 WALLPAPER_CHANGED / WALLPAPER_CLEARED / FIT_MODE_CHANGED / DISPLAY_MODE_CHANGED / VOLUME_CHANGED 事件
 * - 返回当前壁纸、fitMode、displayMode、volume
 */
export function useWallpaperLoader(monitorId: string | null) {
  const [wallpaper, setWallpaper] = useState<Wallpaper | null>(null);
  const [fitMode, setFitMode] = useState<string>("cover");
  const [displayMode, setDisplayMode] = useState<string>("independent");
  const [volume, setVolume] = useState<number>(0);

  // 根据 config 获取壁纸并更新状态
  const loadFromConfig = useCallback(async (config: MonitorConfig) => {
    setFitMode(config.fit_mode || "cover");

    if (!config.wallpaper_id) {
      setWallpaper(null);
      return;
    }

    // 优先从 store 缓存中查找，命中则直接使用
    const storeWallpapers = useWallpaperStore.getState().wallpapers;
    const found = storeWallpapers.find((w) => w.id === config.wallpaper_id) ?? null;
    if (found) {
      setWallpaper(found);
      return;
    }

    // 缓存未命中，通过 getById 接口精确查询单个壁纸
    try {
      const detail = await getWallpaperById(config.wallpaper_id);
      setWallpaper(detail);
    } catch (e) {
      console.error("[useWallpaperLoader] getById failed:", e);
    }
  }, []);

  // 初始化：加载 config + 音量 + displayMode
  useEffect(() => {
    if (!monitorId) return;

    invoke(COMMANDS.GET_SETTING, { key: "display_mode" }, { silent: true }).then((val) => {
      if (val) setDisplayMode(val);
    }).catch(() => {});

    invoke(COMMANDS.GET_MONITOR_CONFIG, { monitorId }, { silent: true }).then((config) => {
      if (config) loadFromConfig(config);
    });

    invoke(COMMANDS.GET_SETTING, { key: "global_volume" }, { silent: true }).then((val) => {
      const v = Number(val ?? "0");
      setVolume(Math.min(Math.max(v, 0), 100));
    }).catch(() => {});
  }, [monitorId, loadFromConfig]);

  // 统一事件监听
  useEffect(() => {
    if (!monitorId) return;

    const unlisteners: Promise<() => void>[] = [];

    unlisteners.push(
      listen(EVENTS.WALLPAPER_CHANGED, (payload) => {
        if (payload.monitor_id === monitorId) {
          invoke(COMMANDS.GET_MONITOR_CONFIG, { monitorId }, { silent: true }).then((config) => {
            if (config) loadFromConfig(config);
          });
        }
      })
    );

    unlisteners.push(
      listen(EVENTS.WALLPAPER_CLEARED, (payload) => {
        if (payload.monitor_id === monitorId) {
          setWallpaper(null);
        }
      })
    );

    unlisteners.push(
      listen(EVENTS.FIT_MODE_CHANGED, (payload) => {
        if (payload.monitor_id === monitorId) {
          setFitMode(payload.fit_mode);
        }
      })
    );

    unlisteners.push(
      listen(EVENTS.DISPLAY_MODE_CHANGED, (payload) => {
        if (payload.monitor_id === monitorId) {
          setDisplayMode(payload.display_mode);
        }
      })
    );

    unlisteners.push(
      listen(EVENTS.VOLUME_CHANGED, (payload) => {
        setVolume(Math.min(Math.max(payload.volume, 0), 100));
      })
    );

    return () => {
      for (const p of unlisteners) {
        p.then((fn) => fn());
      }
    };
  }, [monitorId, loadFromConfig]);

  return { wallpaper, fitMode, displayMode, volume };
}