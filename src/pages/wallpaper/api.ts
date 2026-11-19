import { invoke } from "@tauri-apps/api/core";
import { COMMANDS, type CommandMap } from "@/api/config";

export function getMonitorConfig(
  monitorId: string,
): Promise<CommandMap[typeof COMMANDS.GET_MONITOR_CONFIG]["result"]> {
  return invoke(COMMANDS.GET_MONITOR_CONFIG, { req: { monitorId } });
}

export function getSetting(
  key: string,
): Promise<CommandMap[typeof COMMANDS.GET_SETTING]["result"]> {
  return invoke(COMMANDS.GET_SETTING, { req: { key } });
}

export function getWallpaperById(
  id: number,
): Promise<CommandMap[typeof COMMANDS.GET_WALLPAPER]["result"]> {
  return invoke(COMMANDS.GET_WALLPAPER, { id });
}
