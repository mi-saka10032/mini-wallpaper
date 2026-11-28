import { COMMANDS } from "./config";
import type { AddWatchedFolderResult, WatchedFolderView, WatchedSyncReport } from "./config";
import { invoke } from "./invoke";

/** 目录失效时后端上抛的可识别错误（前端据此 toast + activeId=0） */
export const WATCHED_PATH_INVALID = "WATCHED_PATH_INVALID";

export function listWatchedFolders(): Promise<WatchedFolderView[]> {
  return invoke(COMMANDS.LIST_WATCHED_FOLDERS);
}

export function addWatchedFolder(path: string): Promise<AddWatchedFolderResult> {
  return invoke(COMMANDS.ADD_WATCHED_FOLDER, { path });
}

export function removeWatchedFolder(id: number): Promise<void> {
  return invoke(COMMANDS.REMOVE_WATCHED_FOLDER, { id });
}

/** silent：失效错误由调用方（视图切换 / 手动刷新）自行处理，避免重复 toast */
export function syncWatchedFolder(collectionId: number): Promise<WatchedSyncReport> {
  return invoke(COMMANDS.SYNC_WATCHED_FOLDER, { collectionId }, { silent: true });
}

/** 切视图时通知后端暂停该夹的后台 diff；fire-and-forget，失败不影响 UI */
export function setActiveView(activeId: number): void {
  void invoke(COMMANDS.SET_ACTIVE_VIEW, { activeId }, { silent: true }).catch(() => {});
}

export function deleteWatchedWallpapers(ids: number[]): Promise<number> {
  return invoke(COMMANDS.DELETE_WATCHED_WALLPAPERS, { ids });
}
