import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { COMMANDS, type Wallpaper } from "./config";
import { invoke } from "./invoke";

/** 获取支持的壁纸文件扩展名列表 */
export async function getSupportedExtensions(): Promise<string[]> {
  return invoke(COMMANDS.GET_SUPPORTED_EXTENSIONS);
}

/** 获取所有壁纸 */
export async function getAll(): Promise<Wallpaper[]> {
  return invoke(COMMANDS.GET_WALLPAPERS);
}

/** 根据 ID 获取单个壁纸详情 */
export async function getById(id: number): Promise<Wallpaper | null> {
  return tauriInvoke(COMMANDS.GET_WALLPAPER, { id });
}

/** 导入壁纸文件 */
export async function importFiles(paths: string[]): Promise<Wallpaper[]> {
  return invoke(COMMANDS.IMPORT_WALLPAPERS, { paths });
}

/** 通过字节方式导入单个壁纸文件（H5 拖拽场景）
 *
 * 字节数据通过 Tauri v2 raw body 直传（Uint8Array），避免 JSON 数组
 * 序列化开销；文件名经 fileName 请求头传入（encodeURIComponent 编码）。
 */
export async function importWallpaperBytes(
  name: string,
  data: Uint8Array,
): Promise<Wallpaper> {
  return tauriInvoke(COMMANDS.IMPORT_WALLPAPER_BYTES, data, {
    headers: { fileName: encodeURIComponent(name) },
  });
}

/** 保存视频缩略图（前端 canvas 抽帧后回传字节数据）
 *
 * 字节数据通过 Tauri v2 raw body 直传（Uint8Array），避免 JSON
 * 序列化开销；wallpaperId 经请求头传入。
 */
export async function saveVideoThumbnail(
  wallpaperId: number,
  data: Uint8Array,
): Promise<string> {
  return tauriInvoke(COMMANDS.SAVE_VIDEO_THUMBNAIL, data, {
    headers: { wallpaperId: String(wallpaperId) },
  });
}

/** 批量删除壁纸 —— 语义为「移入回收站」（可恢复，不删磁盘文件） */
export async function deleteBatch(ids: number[]): Promise<number> {
  return invoke(COMMANDS.DELETE_WALLPAPERS, { ids });
}

/** 获取回收站内的壁纸列表（按移入时间倒序） */
export async function getTrashed(): Promise<Wallpaper[]> {
  return invoke(COMMANDS.GET_TRASHED_WALLPAPERS);
}

/** 从回收站恢复壁纸（恢复后追加到原收藏夹末尾） */
export async function restoreBatch(ids: number[]): Promise<number> {
  return invoke(COMMANDS.RESTORE_WALLPAPERS, { ids });
}

/** 彻底删除壁纸（不可恢复：删数据库记录 + 关联 + 磁盘文件） */
export async function purgeBatch(ids: number[]): Promise<number> {
  return invoke(COMMANDS.PURGE_WALLPAPERS, { ids });
}

/** 清空回收站（彻底删除其中全部壁纸，不可恢复） */
export async function emptyTrash(): Promise<number> {
  return invoke(COMMANDS.EMPTY_TRASH);
}