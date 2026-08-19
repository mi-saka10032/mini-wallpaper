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
export async function getCollectionWallpapers(collectionId: number): Promise<Wallpaper[]> {
  return invoke(COMMANDS.GET_COLLECTION_WALLPAPERS, { collectionId });
}

/**
 * 切换壁纸在内置「我喜欢」收藏夹中的收藏状态
 * @returns 切换后的收藏状态：true = 已收藏，false = 已取消收藏
 */
export async function toggleFavorite(wallpaperId: number): Promise<boolean> {
  return invoke(COMMANDS.TOGGLE_FAVORITE, { wallpaperId });
}

/** @deprecated 使用 getCollectionWallpapers 代替 */
export const getWallpapers = getCollectionWallpapers;

/** 创建智能收藏夹（规则经后端白名单校验） */
export async function createSmart(name: string, ruleJson: string): Promise<Collection> {
  return invoke(COMMANDS.CREATE_SMART_COLLECTION, { name, ruleJson });
}

/** 更新智能收藏夹规则（name 传 null 则不改名） */
export async function updateSmart(
  id: number,
  ruleJson: string,
  name?: string | null
): Promise<Collection> {
  return invoke(COMMANDS.UPDATE_SMART_COLLECTION, { id, name: name ?? null, ruleJson });
}

/** 预览一段规则 JSON 的当前命中数（未落库） */
export async function previewSmartCount(ruleJson: string): Promise<number> {
  return invoke(COMMANDS.PREVIEW_SMART_COUNT, { ruleJson });
}