import { COMMANDS, type Collection, type Wallpaper } from "./config";
import { invoke } from "./invoke";

/** 获取所有收藏夹 */
export async function getAll(): Promise<Collection[]> {
  return invoke(COMMANDS.GET_COLLECTIONS);
}

/** 创建收藏夹 */
export async function create(name: string): Promise<Collection> {
  return invoke(COMMANDS.CREATE_COLLECTION, { name });
}

/** 重命名收藏夹 */
export async function rename(id: number, name: string): Promise<Collection> {
  return invoke(COMMANDS.RENAME_COLLECTION, { id, name });
}

/** 删除收藏夹 */
export async function remove(id: number): Promise<void> {
  return invoke(COMMANDS.DELETE_COLLECTION, { id });
}

/** 获取收藏夹内的壁纸列表 */
export async function getWallpapers(collectionId: number): Promise<Wallpaper[]> {
  return invoke(COMMANDS.GET_COLLECTION_WALLPAPERS, { collectionId });
}
