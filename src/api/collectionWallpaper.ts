import { COMMANDS } from "./config";
import { invoke } from "./invoke";

/** 向收藏夹添加壁纸 */
export async function addWallpapers(collectionId: number, wallpaperIds: number[]): Promise<number> {
  return invoke(COMMANDS.ADD_WALLPAPERS_TO_COLLECTION, { collectionId, wallpaperIds });
}

/** 从收藏夹移除壁纸（仅解除关联，不删除文件） */
export async function removeWallpapers(
  collectionId: number,
  wallpaperIds: number[],
): Promise<number> {
  return invoke(COMMANDS.REMOVE_WALLPAPERS_FROM_COLLECTION, { collectionId, wallpaperIds });
}

/** 重新排序收藏夹内的壁纸 */
export async function reorderWallpapers(
  collectionId: number,
  wallpaperIds: number[],
): Promise<void> {
  return invoke(COMMANDS.REORDER_COLLECTION_WALLPAPERS, { collectionId, wallpaperIds });
}
