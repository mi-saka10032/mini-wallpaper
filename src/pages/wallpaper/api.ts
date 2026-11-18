import { invoke } from "@tauri-apps/api/core";
import { COMMANDS, type MonitorConfig, type Wallpaper } from "@/api/config";

export function getMonitorConfig(monitorId: string): Promise<MonitorConfig | null> {
  return invoke(COMMANDS.GET_MONITOR_CONFIG, { req: { monitorId } });
}

export function getSetting(key: string): Promise<string | null> {
  return invoke(COMMANDS.GET_SETTING, { req: { key } });
}

export function getWallpaperById(id: number): Promise<Wallpaper | null> {
  return invoke(COMMANDS.GET_WALLPAPER, { id });
}
