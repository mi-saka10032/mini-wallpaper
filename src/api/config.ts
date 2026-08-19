/** Tauri command 名称常量 */
export const COMMANDS = {
  // wallpaper
  GET_SUPPORTED_EXTENSIONS: "get_supported_extensions",
  GET_WALLPAPERS: "get_wallpapers",
  GET_WALLPAPER: "get_wallpaper",
  IMPORT_WALLPAPERS: "import_wallpapers",
  IMPORT_WALLPAPER_BYTES: "import_wallpaper_bytes",
  SAVE_VIDEO_THUMBNAIL: "save_video_thumbnail",
  DELETE_WALLPAPERS: "delete_wallpapers",
  GET_TRASHED_WALLPAPERS: "get_trashed_wallpapers",
  RESTORE_WALLPAPERS: "restore_wallpapers",
  PURGE_WALLPAPERS: "purge_wallpapers",
  EMPTY_TRASH: "empty_trash",
  // collection
  GET_COLLECTIONS: "get_collections",
  CREATE_COLLECTION: "create_collection",
  RENAME_COLLECTION: "rename_collection",
  DELETE_COLLECTION: "delete_collection",
  GET_COLLECTION_WALLPAPERS: "get_collection_wallpapers",
  CREATE_SMART_COLLECTION: "create_smart_collection",
  UPDATE_SMART_COLLECTION: "update_smart_collection",
  PREVIEW_SMART_COUNT: "preview_smart_count",
  // tag
  GET_TAGS: "get_tags",
  GET_WALLPAPER_TAGS: "get_wallpaper_tags",
  TAG_WALLPAPERS: "tag_wallpapers",
  UNTAG_WALLPAPERS: "untag_wallpapers",
  SET_WALLPAPER_TAGS: "set_wallpaper_tags",
  RENAME_TAG: "rename_tag",
  DELETE_TAG: "delete_tag",
  // collection ↔ wallpaper
  ADD_WALLPAPERS_TO_COLLECTION: "add_wallpapers_to_collection",
  REMOVE_WALLPAPERS_FROM_COLLECTION: "remove_wallpapers_from_collection",
  REORDER_COLLECTION_WALLPAPERS: "reorder_collection_wallpapers",
  TOGGLE_FAVORITE: "toggle_favorite",
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
  play_count: number;
  created_at: string;
  updated_at: string;
  /** 回收站标记：null = 正常；否则为移入回收站的时刻（YYYY-MM-DD HH:mm:ss） */
  deleted_at: string | null;
}

/** 标签模型 */
export interface Tag {
  id: number;
  name: string;
  created_at: string;
}

/** 标签及引用计数（管理 UI 用） */
export interface TagWithCount {
  id: number;
  name: string;
  createdAt: string;
  /** 被多少张壁纸引用 */
  wallpaperCount: number;
}

/** 收藏夹模型 */
export interface Collection {
  id: number;
  name: string;
  sort_order: number;
  created_at: string;
  updated_at: string;
  /** 是否系统内置（1 = 内置「我喜欢」，不可删除/重命名；0 = 用户自建） */
  is_builtin: number;
  /** 收藏夹类型：manual（手动）/ smart（智能收藏夹） */
  kind: "manual" | "smart";
  /** 智能收藏夹规则 JSON（手动收藏夹为 null） */
  rule_json: string | null;
}

/** 显示器配置模型 */
export interface MonitorConfig {
  id: number;
  monitor_id: string;
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

/** 切换壁纸收藏状态请求（内置「我喜欢」收藏夹） */
export interface ToggleFavoriteReq {
  wallpaperId: number;
}

// ---------- 智能收藏夹规则 schema（与后端 smart_rule.rs 对齐）----------

/** 规则组合子 */
export type RuleCombinator = "and" | "or";

/** 规则字段白名单 */
export type RuleField =
  | "tag"
  | "type"
  | "width"
  | "height"
  | "orientation"
  | "created_at"
  | "file_size";

/** 单条规则：字段 + 操作符 + 值（值类型随 field/op 变化） */
export interface RuleItem {
  field: RuleField;
  op: string;
  value: unknown;
}

/** 智能收藏夹规则顶层结构 */
export interface SmartRule {
  version?: number;
  combinator: RuleCombinator;
  rules: RuleItem[];
}

/** 创建智能收藏夹请求 */
export interface CreateSmartCollectionReq {
  name: string;
  ruleJson: string;
}

/** 更新智能收藏夹请求 */
export interface UpdateSmartCollectionReq {
  id: number;
  name?: string | null;
  ruleJson: string;
}

/** 预览规则命中数请求 */
export interface PreviewSmartCountReq {
  ruleJson: string;
}

// ---------- 标签请求 ----------

/** 给一批壁纸打一批标签 */
export interface TagWallpapersReq {
  wallpaperIds: number[];
  tagNames: string[];
}

/** 从一批壁纸移除一批标签（按 id） */
export interface UntagWallpapersReq {
  wallpaperIds: number[];
  tagIds: number[];
}

/** 覆盖式设置单张壁纸标签集合 */
export interface SetWallpaperTagsReq {
  wallpaperId: number;
  tagNames: string[];
}

/** 查某壁纸标签 */
export interface GetWallpaperTagsReq {
  wallpaperId: number;
}

/** 重命名标签 */
export interface RenameTagReq {
  id: number;
  name: string;
}

/** 删除标签 */
export interface DeleteTagReq {
  id: number;
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

/** 设置键值对完整参数（含可选 monitorId） */
export interface SetSettingParams {
  req: SetSettingReq;
  monitorId?: string;
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
  [COMMANDS.GET_WALLPAPER]: {
    params: { id: number };
    result: Wallpaper | null;
  };
  [COMMANDS.IMPORT_WALLPAPERS]: {
    params: { req: ImportWallpapersReq };
    result: Wallpaper[];
  };
  [COMMANDS.IMPORT_WALLPAPER_BYTES]: {
    params: Record<string, never>;
    result: Wallpaper;
  };
  [COMMANDS.SAVE_VIDEO_THUMBNAIL]: {
    params: Record<string, never>;
    result: string;
  };
  [COMMANDS.DELETE_WALLPAPERS]: {
    params: { req: DeleteWallpapersReq };
    result: number;
  };
  [COMMANDS.GET_TRASHED_WALLPAPERS]: {
    params: Record<string, never>;
    result: Wallpaper[];
  };
  [COMMANDS.RESTORE_WALLPAPERS]: {
    params: { req: DeleteWallpapersReq };
    result: number;
  };
  [COMMANDS.PURGE_WALLPAPERS]: {
    params: { req: DeleteWallpapersReq };
    result: number;
  };
  [COMMANDS.EMPTY_TRASH]: {
    params: Record<string, never>;
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
  [COMMANDS.TOGGLE_FAVORITE]: {
    params: { req: ToggleFavoriteReq };
    result: boolean;
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
    params: { req: SetSettingReq; monitorId?: string };
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
  [COMMANDS.CREATE_SMART_COLLECTION]: {
    params: { req: CreateSmartCollectionReq };
    result: Collection;
  };
  [COMMANDS.UPDATE_SMART_COLLECTION]: {
    params: { req: UpdateSmartCollectionReq };
    result: Collection;
  };
  [COMMANDS.PREVIEW_SMART_COUNT]: {
    params: { req: PreviewSmartCountReq };
    result: number;
  };
  [COMMANDS.GET_TAGS]: {
    params: Record<string, never>;
    result: TagWithCount[];
  };
  [COMMANDS.GET_WALLPAPER_TAGS]: {
    params: { req: GetWallpaperTagsReq };
    result: Tag[];
  };
  [COMMANDS.TAG_WALLPAPERS]: {
    params: { req: TagWallpapersReq };
    result: number;
  };
  [COMMANDS.UNTAG_WALLPAPERS]: {
    params: { req: UntagWallpapersReq };
    result: number;
  };
  [COMMANDS.SET_WALLPAPER_TAGS]: {
    params: { req: SetWallpaperTagsReq };
    result: Tag[];
  };
  [COMMANDS.RENAME_TAG]: {
    params: { req: RenameTagReq };
    result: Tag;
  };
  [COMMANDS.DELETE_TAG]: {
    params: { req: DeleteTagReq };
    result: void;
  };
}