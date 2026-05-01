import { useCallback, useState } from "react";
import { useWallpaperStore } from "@/stores/wallpaperStore";
import type { Wallpaper } from "@/api/config";
import { addWallpapers, removeWallpapers } from "@/api/collectionWallpaper";

export interface UseManageModeOptions {
  isCollectionView: boolean;
  collectionId: number | null;
  onCollectionChanged?: () => void;
  onManageModeChange?: (active: boolean) => void;
}

export function useManageMode({
  isCollectionView,
  collectionId,
  onCollectionChanged,
  onManageModeChange,
}: UseManageModeOptions) {
  const deleteWallpapers = useWallpaperStore((s) => s.deleteWallpapers);

  // 管理模式 + 选中状态
  const [manageMode, setManageMode] = useState(false);
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set());
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [pendingDeleteIds, setPendingDeleteIds] = useState<number[]>([]);

  // 收藏夹管理模式下延迟移除的壁纸 ID 列表（退出时批量持久化）
  const [pendingRemovals, setPendingRemovals] = useState<number[]>([]);

  // 本地壁纸管理模式下延迟删除的壁纸 ID 列表（退出时批量持久化）
  const [pendingDeletions, setPendingDeletions] = useState<number[]>([]);

  const enterManageMode = useCallback(() => {
    setManageMode(true);
    setSelectedIds(new Set());
    setPendingRemovals([]);
    setPendingDeletions([]);
    document.body.setAttribute("data-manage-mode", "");
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
    document.body.removeAttribute("data-manage-mode");
    onManageModeChange?.(false);
  }, [isCollectionView, collectionId, pendingRemovals, pendingDeletions, deleteWallpapers, onCollectionChanged, onManageModeChange]);

  const cancelManageMode = useCallback(() => {
    setManageMode(false);
    setSelectedIds(new Set());
    setPendingRemovals([]);
    setPendingDeletions([]);
    document.body.removeAttribute("data-manage-mode");
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

  const selectAll = useCallback((displayWallpapers: Wallpaper[]) => {
    setSelectedIds(new Set(displayWallpapers.map((w) => w.id)));
  }, []);

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

  const handleAddToCollection = useCallback(async (wallpaperId: number, targetCollectionId: number) => {
    try {
      await addWallpapers(targetCollectionId, [wallpaperId]);
    } catch (e) {
      console.error("[addToCollection]", e);
    }
  }, []);

  return {
    manageMode,
    selectedIds,
    deleteDialogOpen,
    pendingDeleteIds,
    pendingRemovals,
    pendingDeletions,
    setDeleteDialogOpen,
    enterManageMode,
    exitManageMode,
    cancelManageMode,
    toggleSelect,
    selectAll,
    clearSelection,
    handleDeleteRequest,
    handleDeleteConfirm,
    handleAddToCollection,
  };
}