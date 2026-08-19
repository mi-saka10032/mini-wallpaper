import {
  COMMANDS,
  type Tag,
  type TagWithCount,
} from "./config";
import { invoke } from "./invoke";

/** 获取全部标签（带引用计数） */
export async function getTags(): Promise<TagWithCount[]> {
  return invoke(COMMANDS.GET_TAGS);
}

/** 获取某壁纸的标签列表 */
export async function getWallpaperTags(wallpaperId: number): Promise<Tag[]> {
  return invoke(COMMANDS.GET_WALLPAPER_TAGS, { wallpaperId });
}

/** 给一批壁纸打一批标签（resolve-or-create + 幂等），返回新增关联条数 */
export async function tagWallpapers(
  wallpaperIds: number[],
  tagNames: string[]
): Promise<number> {
  return invoke(COMMANDS.TAG_WALLPAPERS, { wallpaperIds, tagNames });
}

/** 从一批壁纸移除一批标签（按 id），返回删除关联条数 */
export async function untagWallpapers(
  wallpaperIds: number[],
  tagIds: number[]
): Promise<number> {
  return invoke(COMMANDS.UNTAG_WALLPAPERS, { wallpaperIds, tagIds });
}

/** 覆盖式设置单张壁纸的标签集合，返回设置后完整标签列表 */
export async function setWallpaperTags(
  wallpaperId: number,
  tagNames: string[]
): Promise<Tag[]> {
  return invoke(COMMANDS.SET_WALLPAPER_TAGS, { wallpaperId, tagNames });
}

/** 重命名标签 */
export async function renameTag(id: number, name: string): Promise<Tag> {
  return invoke(COMMANDS.RENAME_TAG, { id, name });
}

/** 删除标签（连带清理关联） */
export async function deleteTag(id: number): Promise<void> {
  return invoke(COMMANDS.DELETE_TAG, { id });
}
