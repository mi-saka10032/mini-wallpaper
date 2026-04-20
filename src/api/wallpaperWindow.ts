import { COMMANDS } from "./config";
import { invoke } from "./invoke";

/** 为指定显示器创建壁纸窗口 */
export async function createWallpaperWindow(
  monitorId: string,
  x: number,
  y: number,
  width: number,
  height: number,
  extraQuery?: string,
): Promise<void> {
  return invoke(COMMANDS.CREATE_WALLPAPER_WINDOW, { monitorId, x, y, width, height, extraQuery });
}

/** 销毁指定显示器的壁纸窗口 */
export async function destroyWallpaperWindow(monitorId: string): Promise<void> {
  return invoke(COMMANDS.DESTROY_WALLPAPER_WINDOW, { monitorId });
}

/** 销毁所有壁纸窗口 */
export async function destroyAllWallpaperWindows(): Promise<void> {
  return invoke(COMMANDS.DESTROY_ALL_WALLPAPER_WINDOWS);
}

/** 隐藏所有壁纸窗口 */
export async function hideWallpaperWindows(): Promise<void> {
  return invoke(COMMANDS.HIDE_WALLPAPER_WINDOWS);
}

/** 显示所有壁纸窗口 */
export async function showWallpaperWindows(): Promise<void> {
  return invoke(COMMANDS.SHOW_WALLPAPER_WINDOWS);
}
