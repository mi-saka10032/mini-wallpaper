/** Tauri command 名称常量 */
export const COMMANDS = {
  // wallpaper
  GET_SUPPORTED_EXTENSIONS: "get_supported_extensions",
  GET_WALLPAPERS: "get_wallpapers",
  IMPORT_WALLPAPERS: "import_wallpapers",
  SAVE_VIDEO_THUMBNAIL: "save_video_thumbnail",
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
  START_TIMERS: "start_timers",
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
  INIT_FULLSCREEN_DETECTION: "init_fullscreen_detection",
  // wallpaper window
  CREATE_WALLPAPER_WINDOW: "create_wallpaper_window",
  DESTROY_WALLPAPER_WINDOW: "destroy_wallpaper_window",
  DESTROY_ALL_WALLPAPER_WINDOWS: "destroy_all_wallpaper_windows",
  HIDE_WALLPAPER_WINDOWS: "hide_wallpaper_windows",
  SHOW_WALLPAPER_WINDOWS: "show_wallpaper_windows",
  GET_ACTIVE_WALLPAPER_WINDOWS: "get_active_wallpaper_windows",
} as const;

// ==================== 实体模型 ====================

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

// ==================== DTO 请求类型 ====================

/** 创建收藏夹请求 */
export interface CreateCollectionReq {
  name: string;
}

/** 重命名收藏夹请求 */
export interface RenameCollectionReq {
  id: number;
  name: string;
}

/** 删除收藏夹请求 */
export interface DeleteCollectionReq {
  id: number;
}

/** 获取收藏夹壁纸请求 */
export interface GetCollectionWallpapersReq {
  collectionId: number;
}

/** 向收藏夹添加壁纸请求 */
export interface AddWallpapersReq {
  collectionId: number;
  wallpaperIds: number[];
}

/** 从收藏夹移除壁纸请求 */
export interface RemoveWallpapersReq {
  collectionId: number;
  wallpaperIds: number[];
}

/** 重新排序收藏夹壁纸请求 */
export interface ReorderWallpapersReq {
  collectionId: number;
  wallpaperIds: number[];
}

/** 导入壁纸请求 */
export interface ImportWallpapersReq {
  paths: string[];
}

/** 保存视频缩略图请求 */
export interface SaveVideoThumbnailReq {
  wallpaperId: number;
  data: number[];
}

/** 批量删除壁纸请求 */
export interface DeleteWallpapersReq {
  ids: number[];
}

/** Upsert 显示器配置请求 */
export interface UpsertMonitorConfigReq {
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
}

/** 获取单个显示器配置请求 */
export interface GetMonitorConfigReq {
  monitorId: string;
}

/** 删除显示器配置请求 */
export interface DeleteMonitorConfigReq {
  id: number;
  monitorId?: string;
}

/** 获取单个设置值请求 */
export interface GetSettingReq {
  key: string;
}

/** 设置键值对请求 */
export interface SetSettingReq {
  key: string;
  value: string;
}

/** 切换壁纸请求 */
export interface SwitchWallpaperReq {
  direction: "next" | "prev";
}

/** 导出备份请求 */
export interface ExportBackupReq {
  outputPath: string;
}

/** 导入备份请求 */
export interface ImportBackupReq {
  zipPath: string;
}

/** 创建壁纸窗口请求 */
export interface CreateWallpaperWindowReq {
  monitorId: string;
  x: number;
  y: number;
  width: number;
  height: number;
  extraQuery?: string;
}

/** 销毁壁纸窗口请求 */
export interface DestroyWallpaperWindowReq {
  monitorId: string;
}

// ==================== Command 入参/出参类型映射 ====================

/** Command 入参/出参类型映射 */
export interface CommandMap {
  [COMMANDS.GET_SUPPORTED_EXTENSIONS]: {
    params: Record<string, never>;
    result: string[];
  };
  [COMMANDS.GET_WALLPAPERS]: {
    params: Record<string, never>;
    result: Wallpaper[];
  };
  [COMMANDS.IMPORT_WALLPAPERS]: {
    params: { req: ImportWallpapersReq };
    result: Wallpaper[];
  };
  [COMMANDS.SAVE_VIDEO_THUMBNAIL]: {
    params: { wallpaperId: number; data: number[] };
    result: string;
  };
  [COMMANDS.DELETE_WALLPAPERS]: {
    params: { req: DeleteWallpapersReq };
    result: number;
  };
  [COMMANDS.GET_COLLECTIONS]: {
    params: Record<string, never>;
    result: Collection[];
  };
  [COMMANDS.CREATE_COLLECTION]: {
    params: { req: CreateCollectionReq };
    result: Collection;
  };
  [COMMANDS.RENAME_COLLECTION]: {
    params: { req: RenameCollectionReq };
    result: Collection;
  };
  [COMMANDS.DELETE_COLLECTION]: {
    params: { req: DeleteCollectionReq };
    result: void;
  };
  [COMMANDS.GET_COLLECTION_WALLPAPERS]: {
    params: { req: GetCollectionWallpapersReq };
    result: Wallpaper[];
  };
  [COMMANDS.ADD_WALLPAPERS_TO_COLLECTION]: {
    params: { req: AddWallpapersReq };
    result: number;
  };
  [COMMANDS.REMOVE_WALLPAPERS_FROM_COLLECTION]: {
    params: { req: RemoveWallpapersReq };
    result: number;
  };
  [COMMANDS.REORDER_COLLECTION_WALLPAPERS]: {
    params: { req: ReorderWallpapersReq };
    result: void;
  };
  [COMMANDS.GET_MONITOR_CONFIGS]: {
    params: Record<string, never>;
    result: MonitorConfig[];
  };
  [COMMANDS.GET_MONITOR_CONFIG]: {
    params: { req: GetMonitorConfigReq };
    result: MonitorConfig | null;
  };
  [COMMANDS.UPSERT_MONITOR_CONFIG]: {
    params: { req: UpsertMonitorConfigReq };
    result: MonitorConfig;
  };
  [COMMANDS.DELETE_MONITOR_CONFIG]: {
    params: { req: DeleteMonitorConfigReq };
    result: void;
  };
  [COMMANDS.START_TIMERS]: {
    params: Record<string, never>;
    result: void;
  };
  [COMMANDS.GET_SETTINGS]: {
    params: Record<string, never>;
    result: Record<string, string>;
  };
  [COMMANDS.GET_SETTING]: {
    params: { req: GetSettingReq };
    result: string | null;
  };
  [COMMANDS.SET_SETTING]: {
    params: { req: SetSettingReq };
    result: void;
  };
  [COMMANDS.SWITCH_WALLPAPER]: {
    params: { req: SwitchWallpaperReq };
    result: void;
  };
  [COMMANDS.EXPORT_BACKUP]: {
    params: { req: ExportBackupReq };
    result: string;
  };
  [COMMANDS.IMPORT_BACKUP]: {
    params: { req: ImportBackupReq };
    result: number;
  };
  [COMMANDS.GET_DATA_SIZE]: {
    params: Record<string, never>;
    result: number;
  };
  [COMMANDS.INIT_FULLSCREEN_DETECTION]: {
    params: Record<string, never>;
    result: void;
  };
  [COMMANDS.CREATE_WALLPAPER_WINDOW]: {
    params: { req: CreateWallpaperWindowReq };
    result: void;
  };
  [COMMANDS.DESTROY_WALLPAPER_WINDOW]: {
    params: { req: DestroyWallpaperWindowReq };
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
  [COMMANDS.GET_ACTIVE_WALLPAPER_WINDOWS]: {
    params: Record<string, never>;
    result: string[];
  };
}