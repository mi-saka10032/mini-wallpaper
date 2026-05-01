import { useCallback, useEffect, useRef, useState } from "react";
import type { Wallpaper } from "@/api/config";
import { useWallpaperStore } from "@/stores/wallpaperStore";
import { sortWallpapers } from "@/utils/sort";
import type { SortField, SortOrder } from "@/utils/sort";

export interface UseWallpaperSearchOptions {
  activeId: number;
}

/** 通用壁纸过滤工具函数：关键词过滤 + 可选排序 */
function filterWallpapers(
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
    const isDefault = sortField === "created_at" && sortOrder === "desc";
    if (!isDefault) {
      result = sortWallpapers(result, sortField, sortOrder);
    }
  }

  return result;
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
      return filterWallpapers(source, keyword, sortField, sortOrder);
    },
    [keyword, sortField, sortOrder],
  );

  /**
   * 计算常态模式下基于 normalKeyword 过滤的壁纸列表
   */
  const getNormalFilteredWallpapers = useCallback(
    (wallpapers: Wallpaper[]): Wallpaper[] => {
      if (!normalKeyword.trim()) return wallpapers;
      return filterWallpapers(wallpapers, normalKeyword);
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