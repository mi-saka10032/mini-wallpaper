import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { DragEndEvent } from "@dnd-kit/core";
import { useWallpaperStore } from "@/stores/wallpaperStore";
import type { Wallpaper } from "@/stores/wallpaperStore";
import { sortWallpapers } from "@/components/wallpaper/WallpaperGrid";
import type { SortField, SortOrder } from "@/components/wallpaper/WallpaperGrid";
import { addWallpapers, removeWallpapers, reorderWallpapers } from "@/api/collectionWallpaper";

export interface UseMainContentOptions {
  activeId: number;
  wallpapers: Wallpaper[];
  onPreview: (index: number) => void;
  onCollectionChanged?: () => void;
  onManageModeChange?: (active: boolean) => void;
}

export function useMainContent({
  activeId,
  wallpapers,
  onPreview,
  onCollectionChanged,
  onManageModeChange,
}: UseMainContentOptions) {
  const loading = useWallpaperStore((s) => s.loading);
  const deleteWallpapers = useWallpaperStore((s) => s.deleteWallpapers);

  // 管理模式 + 选中状态
  const [manageMode, setManageMode] = useState(false);
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set());
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [pendingDeleteIds, setPendingDeleteIds] = useState<number[]>([]);

  // 拖拽排序模式（独立于管理模式，仅收藏夹视图可用）
  const [sortMode, setSortMode] = useState(false);

  // 拖拽排序：本地排序状态（仅排序模式下使用）
  const [localOrder, setLocalOrder] = useState<Wallpaper[] | null>(null);
  const [orderDirty, setOrderDirty] = useState(false);

  // 收藏夹管理模式下延迟移除的壁纸 ID 列表（退出时批量持久化）
  const [pendingRemovals, setPendingRemovals] = useState<number[]>([]);

  // 本地壁纸管理模式下延迟删除的壁纸 ID 列表（退出时批量持久化）
  const [pendingDeletions, setPendingDeletions] = useState<number[]>([]);

  // 添加壁纸到收藏夹的 picker
  const [pickerOpen, setPickerOpen] = useState(false);

  // 搜索 + 排序状态（管理模式）
  const [keyword, setKeyword] = useState("");
  const [sortField, setSortField] = useState<SortField>("created_at");
  const [sortOrder, setSortOrder] = useState<SortOrder>("desc");

  // 常态模式搜索
  const [normalKeyword, setNormalKeyword] = useState("");
  const [searchExpanded, setSearchExpanded] = useState(false);

  const isCollectionView = activeId > 0;
  const collectionId = isCollectionView ? activeId : null;
  const isEmpty = wallpapers.length === 0;

  // 搜索 + 排序后的壁纸列表（仅管理模式下使用）
  const filteredWallpapers = useMemo(() => {
    if (!manageMode) return wallpapers;
    let source = wallpapers;
    if (isCollectionView && pendingRemovals.length > 0) {
      source = source.filter((w) => !pendingRemovals.includes(w.id));
    }
    if (!isCollectionView && pendingDeletions.length > 0) {
      source = source.filter((w) => !pendingDeletions.includes(w.id));
    }

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
  }, [wallpapers, manageMode, isCollectionView, pendingRemovals, pendingDeletions, keyword, sortField, sortOrder]);

  // 常态模式下基于 normalKeyword 过滤的壁纸列表
  const normalFilteredWallpapers = useMemo(() => {
    if (manageMode || sortMode) return wallpapers;
    const kw = normalKeyword.trim().toLowerCase();
    if (!kw) return wallpapers;
    return wallpapers.filter((w) => w.name.toLowerCase().includes(kw));
  }, [wallpapers, manageMode, sortMode, normalKeyword]);

  // 排序模式下使用 localOrder；管理模式下使用过滤排序后的列表；常态下使用 normalKeyword 过滤
  const displayWallpapers = sortMode && localOrder
    ? localOrder
    : manageMode
      ? filteredWallpapers
      : normalFilteredWallpapers;

  const wallpaperIds = useMemo(() => displayWallpapers.map((w) => w.id), [displayWallpapers]);

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

  const enterManageMode = useCallback(() => {
    setManageMode(true);
    setSelectedIds(new Set());
    setPendingRemovals([]);
    setPendingDeletions([]);
    setKeyword("");
    setNormalKeyword("");
    setSearchExpanded(false);
    onManageModeChange?.(true);
  }, [onManageModeChange]);

  const exitManageMode = useCallback(async () => {
    if (isCollectionView && collectionId !== null && pendingRemovals.length > 0) {
      try {
        await removeWallpapers(collectionId, pendingRemovals);
        onCollectionChanged?.();
      } catch (e) {
        console.error("[exitManageMode]", e);
      }
    }
    if (!isCollectionView && pendingDeletions.length > 0) {
      try {
        await deleteWallpapers(pendingDeletions);
      } catch (e) {
        console.error("[exitManageMode] delete wallpapers failed:", e);
      }
    }
    setManageMode(false);
    setSelectedIds(new Set());
    setPendingRemovals([]);
    setPendingDeletions([]);
    setKeyword("");
    onManageModeChange?.(false);
  }, [isCollectionView, collectionId, pendingRemovals, pendingDeletions, deleteWallpapers, onCollectionChanged, onManageModeChange]);

  const cancelManageMode = useCallback(() => {
    setManageMode(false);
    setSelectedIds(new Set());
    setPendingRemovals([]);
    setPendingDeletions([]);
    setKeyword("");
    onManageModeChange?.(false);
  }, [onManageModeChange]);

  const toggleSelect = useCallback((id: number) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const selectAll = useCallback(() => {
    setSelectedIds(new Set(displayWallpapers.map((w) => w.id)));
  }, [displayWallpapers]);

  const clearSelection = useCallback(() => {
    setSelectedIds(new Set());
  }, []);

  const handleDeleteRequest = useCallback((ids: number[]) => {
    if (manageMode) {
      if (isCollectionView) {
        setPendingRemovals((prev) => [...prev, ...ids]);
      } else {
        setPendingDeletions((prev) => [...prev, ...ids]);
      }
      setSelectedIds((prev) => {
        const next = new Set(prev);
        ids.forEach((id) => next.delete(id));
        return next;
      });
    } else {
      setPendingDeleteIds(ids);
      setDeleteDialogOpen(true);
    }
  }, [manageMode, isCollectionView]);

  const handleDeleteConfirm = useCallback(async () => {
    if (isCollectionView && collectionId !== null) {
      await removeWallpapers(collectionId, pendingDeleteIds);
      onCollectionChanged?.();
    } else {
      await deleteWallpapers(pendingDeleteIds);
    }
    setPendingDeleteIds([]);
    setDeleteDialogOpen(false);
    setSelectedIds(new Set());
  }, [deleteWallpapers, pendingDeleteIds, isCollectionView, collectionId, onCollectionChanged]);

  const handleCardClick = useCallback(
    (wp: Wallpaper, _index: number, _e: React.MouseEvent) => {
      if (sortMode) return;
      if (manageMode) {
        toggleSelect(wp.id);
      } else {
        const realIndex = wallpapers.findIndex((w) => w.id === wp.id);
        onPreview(realIndex !== -1 ? realIndex : _index);
      }
    },
    [sortMode, manageMode, toggleSelect, onPreview, wallpapers],
  );

  const handleAddToCollection = useCallback(async (wallpaperId: number, targetCollectionId: number) => {
    try {
      await addWallpapers(targetCollectionId, [wallpaperId]);
    } catch (e) {
      console.error("[addToCollection]", e);
    }
  }, []);

  const handlePickerConfirm = useCallback(() => {
    setPickerOpen(false);
    setNormalKeyword("");
    setSearchExpanded(false);
    onCollectionChanged?.();
  }, [onCollectionChanged]);

  // ===== 排序模式 =====
  const enterSortMode = useCallback(() => {
    setSortMode(true);
    setLocalOrder([...wallpapers]);
    setOrderDirty(false);
    setNormalKeyword("");
    setSearchExpanded(false);
    onManageModeChange?.(true);
  }, [wallpapers, onManageModeChange]);

  const exitSortMode = useCallback(async () => {
    if (collectionId !== null && orderDirty && localOrder) {
      try {
        await reorderWallpapers(collectionId, localOrder.map((w) => w.id));
        onCollectionChanged?.();
      } catch (e) {
        console.error("[exitSortMode]", e);
      }
    }
    setSortMode(false);
    setLocalOrder(null);
    setOrderDirty(false);
    onManageModeChange?.(false);
  }, [collectionId, orderDirty, localOrder, onCollectionChanged, onManageModeChange]);

  const cancelSortMode = useCallback(() => {
    setSortMode(false);
    setLocalOrder(null);
    setOrderDirty(false);
    onManageModeChange?.(false);
  }, [onManageModeChange]);

  // 拖拽结束：重排本地列表
  const handleDragEnd = useCallback(
    (event: DragEndEvent) => {
      const { active, over } = event;
      if (!over || active.id === over.id || !localOrder) return;

      const oldIndex = localOrder.findIndex((w) => w.id === active.id);
      const newIndex = localOrder.findIndex((w) => w.id === over.id);
      if (oldIndex === -1 || newIndex === -1) return;

      const updated = [...localOrder];
      const [moved] = updated.splice(oldIndex, 1);
      updated.splice(newIndex, 0, moved);
      setLocalOrder(updated);
      setOrderDirty(true);
    },
    [localOrder],
  );

  // 是否启用拖拽排序（仅排序模式下启用）
  const isDragEnabled = sortMode && localOrder !== null;

  return {
    // 状态
    loading,
    manageMode,
    sortMode,
    selectedIds,
    deleteDialogOpen,
    pendingDeleteIds,
    localOrder,
    orderDirty,
    pendingRemovals,
    pendingDeletions,
    pickerOpen,
    keyword,
    sortField,
    sortOrder,
    normalKeyword,
    searchExpanded,
    isCollectionView,
    collectionId,
    isEmpty,
    displayWallpapers,
    wallpaperIds,
    isDragEnabled,

    // 状态设置器
    setDeleteDialogOpen,
    setPickerOpen,
    setKeyword,
    setSortField,
    setSortOrder,
    setNormalKeyword,
    setSearchExpanded,

    // 操作
    enterManageMode,
    exitManageMode,
    cancelManageMode,
    toggleSelect,
    selectAll,
    clearSelection,
    handleDeleteRequest,
    handleDeleteConfirm,
    handleCardClick,
    handleAddToCollection,
    handlePickerConfirm,
    enterSortMode,
    exitSortMode,
    cancelSortMode,
    handleDragEnd,
  };
}
