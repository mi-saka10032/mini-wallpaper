import { COMMANDS, type Wallpaper } from "./config";
import { invoke } from "./invoke";

/** 获取所有壁纸 */
export async function getAll(): Promise<Wallpaper[]> {
  return invoke(COMMANDS.GET_WALLPAPERS);
}

/** 导入壁纸文件 */
export async function importFiles(paths: string[]): Promise<Wallpaper[]> {
  return invoke(COMMANDS.IMPORT_WALLPAPERS, { paths });
}

/** 批量删除壁纸（彻底删除文件+缩略图+数据库记录） */
export async function deleteBatch(ids: number[]): Promise<number> {
  return invoke(COMMANDS.DELETE_WALLPAPERS, { ids });
}
