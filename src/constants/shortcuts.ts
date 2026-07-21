/**
 * 默认快捷键常量
 *
 * 快捷键的注册与响应已全部收归后端（`platform::global_shortcut`），
 * 前端仅负责键位的显示、录制、重置等 CRUD，并通过 `set_setting` 落库；
 * 落库后由后端副作用整组重注册使新键位生效。
 *
 * 此处仅保留默认键位常量，供设置面板展示与"重置默认"使用。
 * 键位字符串需与后端 `DEFAULTS` 中的 accelerator 语义保持一致。
 */
export const DEFAULT_SHORTCUTS = {
  nextWallpaper: "CommandOrControl+Alt+Right",
  prevWallpaper: "CommandOrControl+Alt+Left",
  togglePause: "CommandOrControl+Alt+Space",
  openMain: "CommandOrControl+Alt+W",
  toggleFavorite: "CommandOrControl+Alt+F",
} as const;
