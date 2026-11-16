import type { Wallpaper } from "@/api/config";
import { sortWallpapers, type SortField, type SortOrder } from "@/lib/sort";

/**
 * 壁纸列表的关键词过滤 + 可选排序。
 *
 * 主内容区（管理模式带排序、常态模式只按关键词过滤）与收藏夹选择器
 * 共用这一份实现，避免两处各自维护 name 匹配规则与排序时机。
 * `sortField` / `sortOrder` 同时给出时才排序，缺一个就只做过滤。
 */
export function filterWallpapers(
  source: Wallpaper[],
  keyword: string,
  sortField?: SortField,
  sortOrder?: SortOrder,
): Wallpaper[] {
  let result = source;

  const kw = keyword.trim().toLowerCase();
  if (kw) {
    result = result.filter((w) => w.name.toLowerCase().includes(kw));
  }

  if (sortField && sortOrder) {
    result = sortWallpapers(result, sortField, sortOrder);
  }

  return result;
}
