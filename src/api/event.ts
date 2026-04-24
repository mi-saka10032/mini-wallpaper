import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";

/** Tauri 事件名常量 */
export const EVENTS = {
  WALLPAPER_CHANGED: "wallpaper-changed",
  /** 主窗口缩略图更新事件（由 timer_manager 轮播切换时发送给 app_handle） */
  THUMBNAIL_CHANGED: "thumbnail-changed",
  BACKUP_PROGRESS: "backup-progress",
  FULLSCREEN_CHANGED: "fullscreen-changed",
  /** extend 模式视频同步：master 窗口广播 currentTime，slave 窗口对齐 */
  VIDEO_SYNC: "video-sync",
} as const;

/** 壁纸变更事件 payload */
export interface WallpaperChangedPayload {
  monitor_id: string;
  wallpaper_id: number;
}

/** 缩略图变更事件 payload（主窗口使用） */
export interface ThumbnailChangedPayload {
  monitor_id: string;
  wallpaper_id: number;
}

/** 备份进度事件 payload */
export interface BackupProgressPayload {
  current: number;
  total: number;
}

/** 全屏状态变更事件 payload */
export interface FullscreenChangedPayload {
  is_fullscreen: boolean;
}

/** 视频同步事件 payload（extend 模式跨窗口帧同步） */
export interface VideoSyncPayload {
  current_time: number;
}

/** 事件 → payload 类型映射 */
export interface EventMap {
  [EVENTS.WALLPAPER_CHANGED]: WallpaperChangedPayload;
  [EVENTS.THUMBNAIL_CHANGED]: ThumbnailChangedPayload;
  [EVENTS.BACKUP_PROGRESS]: BackupProgressPayload;
  [EVENTS.FULLSCREEN_CHANGED]: FullscreenChangedPayload;
  [EVENTS.VIDEO_SYNC]: VideoSyncPayload;
}

/**
 * 类型安全的 listen 封装
 * 事件名和 payload 类型由 EventMap 自动推导
 */
export function listen<K extends keyof EventMap>(
  event: K,
  handler: (payload: EventMap[K]) => void,
): Promise<UnlistenFn> {
  return tauriListen<EventMap[K]>(event, (e) => handler(e.payload));
}