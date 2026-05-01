import { useCallback, useEffect, useRef, useState } from "react";
import type { Wallpaper } from "@/stores/wallpaperStore";
import { useWallpaperStore } from "@/stores/wallpaperStore";
import { sortWallpapers } from "@/components/wallpaper/WallpaperGrid";
import type { SortField, SortOrder } from "@/components/wallpaper/WallpaperGrid";

export interface UseWallpaperSearchOptions {
  activeId: number;
}

export function useWallpaperSearch({ activeId }: UseWallpaperSearchOptions) {
  const loading = useWallpaperStore((s) => s.loading);

  // 管理模式搜索 + 排序状态
  const [keyword, setKeyword] = useState("");
  const [sortField, setSortField] = useState<SortField>("created_at");
  const [sortOrder, setSortOrder] = useState<SortOrder>("desc");

  // 常态模式搜索
  const [normalKeyword, setNormalKeyword] = useState("");
  const [searchExpanded, setSearchExpanded] = useState(false);

  // activeId 切换时清理常态搜索词
  useEffect(() => {
    setNormalKeyword("");
    setSearchExpanded(false);
  }, [activeId]);

  // 导入完成时（loading: true → false）清理常态搜索词
  const prevLoadingRef = useRef(loading);
  useEffect(() => {
    if (prevLoadingRef.current && !loading) {
      setNormalKeyword("");
      setSearchExpanded(false);
    }
    prevLoadingRef.current = loading;
  }, [loading]);

  /** 重置管理模式搜索状态 */
  const resetManageSearch = useCallback(() => {
    setKeyword("");
  }, []);

  /** 重置常态搜索状态 */
  const resetNormalSearch = useCallback(() => {
    setNormalKeyword("");
    setSearchExpanded(false);
  }, []);

  /**
   * 计算管理模式下过滤+排序后的壁纸列表
   * 需要外部传入 source（已排除 pending 的壁纸列表）
   */
  const getFilteredWallpapers = useCallback(
    (source: Wallpaper[]): Wallpaper[] => {
      const hasKeyword = keyword.trim().length > 0;
      const isDefault = sortField === "created_at" && sortOrder === "desc";
      if (!hasKeyword && isDefault) return source;

      let result = source;
      if (hasKeyword) {
        const kw = keyword.trim().toLowerCase();
        result = result.filter((w) => w.name.toLowerCase().includes(kw));
      }
      if (!isDefault) {
        result = sortWallpapers(result, sortField, sortOrder);
      }
      return result;
    },
    [keyword, sortField, sortOrder],
  );

  /**
   * 计算常态模式下基于 normalKeyword 过滤的壁纸列表
   */
  const getNormalFilteredWallpapers = useCallback(
    (wallpapers: Wallpaper[]): Wallpaper[] => {
      const kw = normalKeyword.trim().toLowerCase();
      if (!kw) return wallpapers;
      return wallpapers.filter((w) => w.name.toLowerCase().includes(kw));
    },
    [normalKeyword],
  );

  return {
    loading,
    keyword,
    sortField,
    sortOrder,
    normalKeyword,
    searchExpanded,
    setKeyword,
    setSortField,
    setSortOrder,
    setNormalKeyword,
    setSearchExpanded,
    resetManageSearch,
    resetNormalSearch,
    getFilteredWallpapers,
    getNormalFilteredWallpapers,
  };
}