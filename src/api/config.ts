/** Tauri command 名称常量 */
export const COMMANDS = {
  // wallpaper
  GET_WALLPAPERS: "get_wallpapers",
  IMPORT_WALLPAPERS: "import_wallpapers",
  DELETE_WALLPAPERS: "delete_wallpapers",
  // collection
  GET_COLLECTIONS: "get_collections",
  CREATE_COLLECTION: "create_collection",
  RENAME_COLLECTION: "rename_collection",
  DELETE_COLLECTION: "delete_collection",
  GET_COLLECTION_WALLPAPERS: "get_collection_wallpapers",
  // collection ↔ wallpaper
  ADD_WALLPAPERS_TO_COLLECTION: "add_wallpapers_to_collection",
  REMOVE_WALLPAPERS_FROM_COLLECTION: "remove_wallpapers_from_collection",
  REORDER_COLLECTION_WALLPAPERS: "reorder_collection_wallpapers",
  // monitor_config
  GET_MONITOR_CONFIGS: "get_monitor_configs",
  GET_MONITOR_CONFIG: "get_monitor_config",
  UPSERT_MONITOR_CONFIG: "upsert_monitor_config",
  DELETE_MONITOR_CONFIG: "delete_monitor_config",
  // app_setting
  GET_SETTINGS: "get_settings",
  GET_SETTING: "get_setting",
  SET_SETTING: "set_setting",
  // shortcut
  SWITCH_WALLPAPER: "switch_wallpaper",
  // backup
  EXPORT_BACKUP: "export_backup",
  IMPORT_BACKUP: "import_backup",
  GET_DATA_SIZE: "get_data_size",
  // fullscreen
  SET_FULLSCREEN_DETECTION: "set_fullscreen_detection",
  // wallpaper window
  CREATE_WALLPAPER_WINDOW: "create_wallpaper_window",
  DESTROY_WALLPAPER_WINDOW: "destroy_wallpaper_window",
  DESTROY_ALL_WALLPAPER_WINDOWS: "destroy_all_wallpaper_windows",
  HIDE_WALLPAPER_WINDOWS: "hide_wallpaper_windows",
  SHOW_WALLPAPER_WINDOWS: "show_wallpaper_windows",
} as const;

/** 壁纸模型 */
export interface Wallpaper {
  id: number;
  name: string;
  type: "image" | "video" | "gif";
  file_path: string;
  thumb_path: string | null;
  width: number | null;
  height: number | null;
  duration: number | null;
  file_size: number | null;
  tags: string | null;
  is_favorite: number;
  play_count: number;
  created_at: string;
  updated_at: string;
}

/** 收藏夹模型 */
export interface Collection {
  id: number;
  name: string;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

/** 显示器配置模型 */
export interface MonitorConfig {
  id: number;
  monitor_id: string;
  display_mode: string;
  wallpaper_id: number | null;
  collection_id: number | null;
  fit_mode: string;
  play_mode: string;
  play_interval: number;
  is_enabled: boolean;
  active: boolean;
  updated_at: string;
}

/** Command 入参/出参类型映射 */
export interface CommandMap {
  [COMMANDS.GET_WALLPAPERS]: {
    params: Record<string, never>;
    result: Wallpaper[];
  };
  [COMMANDS.IMPORT_WALLPAPERS]: {
    params: { paths: string[] };
    result: Wallpaper[];
  };
  [COMMANDS.DELETE_WALLPAPERS]: {
    params: { ids: number[] };
    result: number;
  };
  [COMMANDS.GET_COLLECTIONS]: {
    params: Record<string, never>;
    result: Collection[];
  };
  [COMMANDS.CREATE_COLLECTION]: {
    params: { name: string };
    result: Collection;
  };
  [COMMANDS.RENAME_COLLECTION]: {
    params: { id: number; name: string };
    result: Collection;
  };
  [COMMANDS.DELETE_COLLECTION]: {
    params: { id: number };
    result: void;
  };
  [COMMANDS.GET_COLLECTION_WALLPAPERS]: {
    params: { collectionId: number };
    result: Wallpaper[];
  };
  [COMMANDS.ADD_WALLPAPERS_TO_COLLECTION]: {
    params: { collectionId: number; wallpaperIds: number[] };
    result: number;
  };
  [COMMANDS.REMOVE_WALLPAPERS_FROM_COLLECTION]: {
    params: { collectionId: number; wallpaperIds: number[] };
    result: number;
  };
  [COMMANDS.REORDER_COLLECTION_WALLPAPERS]: {
    params: { collectionId: number; wallpaperIds: number[] };
    result: void;
  };
  [COMMANDS.GET_MONITOR_CONFIGS]: {
    params: Record<string, never>;
    result: MonitorConfig[];
  };
  [COMMANDS.GET_MONITOR_CONFIG]: {
    params: { monitorId: string };
    result: MonitorConfig | null;
  };
  [COMMANDS.UPSERT_MONITOR_CONFIG]: {
    params: {
      monitorId: string;
      wallpaperId?: number | null;
      collectionId?: number | null;
      clearCollection?: boolean;
      displayMode?: string;
      fitMode?: string;
      playMode?: string;
      playInterval?: number;
      isEnabled?: boolean;
      active?: boolean;
    };
    result: MonitorConfig;
  };
  [COMMANDS.DELETE_MONITOR_CONFIG]: {
    params: { id: number };
    result: void;
  };
  [COMMANDS.GET_SETTINGS]: {
    params: Record<string, never>;
    result: Record<string, string>;
  };
  [COMMANDS.GET_SETTING]: {
    params: { key: string };
    result: string | null;
  };
  [COMMANDS.SET_SETTING]: {
    params: { key: string; value: string };
    result: void;
  };
  [COMMANDS.SWITCH_WALLPAPER]: {
    params: { direction: "next" | "prev" };
    result: void;
  };
  [COMMANDS.EXPORT_BACKUP]: {
    params: { outputPath: string };
    result: string;
  };
  [COMMANDS.IMPORT_BACKUP]: {
    params: { zipPath: string };
    result: number;
  };
  [COMMANDS.GET_DATA_SIZE]: {
    params: Record<string, never>;
    result: number;
  };
  [COMMANDS.SET_FULLSCREEN_DETECTION]: {
    params: { enabled: boolean };
    result: void;
  };
  [COMMANDS.CREATE_WALLPAPER_WINDOW]: {
    params: { monitorId: string; x: number; y: number; width: number; height: number; extraQuery?: string };
    result: void;
  };
  [COMMANDS.DESTROY_WALLPAPER_WINDOW]: {
    params: { monitorId: string };
    result: void;
  };
  [COMMANDS.DESTROY_ALL_WALLPAPER_WINDOWS]: {
    params: Record<string, never>;
    result: void;
  };
  [COMMANDS.HIDE_WALLPAPER_WINDOWS]: {
    params: Record<string, never>;
    result: void;
  };
  [COMMANDS.SHOW_WALLPAPER_WINDOWS]: {
    params: Record<string, never>;
    result: void;
  };
}
