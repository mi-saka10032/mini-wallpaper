import type { Wallpaper } from "@/api/config";

export type SortField = "name" | "created_at" | "file_size" | "type";
export type SortOrder = "asc" | "desc";

/**
 * 对壁纸列表进行排序
 * @param wallpapers 壁纸数组
 * @param field 排序字段
 * @param order 排序方向
 */
export function sortWallpapers(
  wallpapers: Wallpaper[],
  field: SortField,
  order: SortOrder,
): Wallpaper[] {
  const sorted = [...wallpapers].sort((a, b) => {
    let cmp = 0;
    switch (field) {
      case "name":
        cmp = a.name.localeCompare(b.name, undefined, { numeric: true });
        break;
      case "created_at":
        cmp = a.created_at.localeCompare(b.created_at);
        break;
      case "file_size":
        cmp = (a.file_size ?? 0) - (b.file_size ?? 0);
        break;
      case "type":
        cmp = a.type.localeCompare(b.type);
        break;
    }
    return order === "asc" ? cmp : -cmp;
  });
  return sorted;
}
