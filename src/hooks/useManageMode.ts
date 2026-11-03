import { useCallback, useRef, useState } from "react";
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

  // 用 ref 持有 pending 数据，使 exitManageMode 引用稳定
  const pendingRemovalsRef = useRef(pendingRemovals);
  pendingRemovalsRef.current = pendingRemovals;
  const pendingDeletionsRef = useRef(pendingDeletions);
  pendingDeletionsRef.current = pendingDeletions;
  const pendingDeleteIdsRef = useRef(pendingDeleteIds);
  pendingDeleteIdsRef.current = pendingDeleteIds;

  // 同步锁：完成和确认删除共用，挡住状态更新前的连点
  const submittingRef = useRef(false);
  const [submitting, setSubmitting] = useState(false);
  const beginSubmit = useCallback(() => {
    if (submittingRef.current) return false;
    submittingRef.current = true;
    setSubmitting(true);
    return true;
  }, []);
  const endSubmit = useCallback(() => {
    submittingRef.current = false;
    setSubmitting(false);
  }, []);

  const enterManageMode = useCallback(() => {
    setManageMode(true);
    setSelectedIds(new Set());
    setPendingRemovals([]);
    setPendingDeletions([]);
    document.body.setAttribute("data-manage-mode", "");
    onManageModeChange?.(true);
  }, [onManageModeChange]);

  const exitManageMode = useCallback(async () => {
    if (!beginSubmit()) return false;
    try {
      if (isCollectionView && collectionId !== null && pendingRemovalsRef.current.length > 0) {
        await removeWallpapers(collectionId, pendingRemovalsRef.current);
        onCollectionChanged?.();
      }
      if (!isCollectionView && pendingDeletionsRef.current.length > 0) {
        await deleteWallpapers(pendingDeletionsRef.current);
        // 与 handleDeleteConfirm 同理：软删除影响收藏夹命中集，需刷新当前视图
        onCollectionChanged?.();
      }
      pendingRemovalsRef.current = [];
      pendingDeletionsRef.current = [];
      setManageMode(false);
      setSelectedIds(new Set());
      setPendingRemovals([]);
      setPendingDeletions([]);
      document.body.removeAttribute("data-manage-mode");
      onManageModeChange?.(false);
      return true;
    } catch (e) {
      console.error("[exitManageMode]", e);
      return false;
    } finally {
      endSubmit();
    }
  }, [
    isCollectionView,
    collectionId,
    deleteWallpapers,
    onCollectionChanged,
    onManageModeChange,
    beginSubmit,
    endSubmit,
  ]);

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

  // 用 ref 持有 manageMode 和 isCollectionView，使 handleDeleteRequest 引用稳定
  const manageModeRef = useRef(manageMode);
  manageModeRef.current = manageMode;
  const isCollectionViewRef = useRef(isCollectionView);
  isCollectionViewRef.current = isCollectionView;

  const handleDeleteRequest = useCallback((ids: number[]) => {
    if (manageModeRef.current) {
      if (isCollectionViewRef.current) {
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
  }, []);

  const handleDeleteConfirm = useCallback(async () => {
    if (!beginSubmit()) return;
    const ids = pendingDeleteIdsRef.current;
    try {
      if (isCollectionView && collectionId !== null) {
        await removeWallpapers(collectionId, ids);
        onCollectionChanged?.();
      } else {
        await deleteWallpapers(ids);
        // 软删除会改变收藏夹命中集（智能收藏夹实时求值、手动收藏夹过滤已删成员），
        // 若当前正停留在收藏夹视图，需重新拉取，否则已入回收站的壁纸仍留在网格上
        onCollectionChanged?.();
      }
      pendingDeleteIdsRef.current = [];
      setPendingDeleteIds([]);
      setDeleteDialogOpen(false);
      setSelectedIds(new Set());
    } catch (e) {
      console.error("[handleDeleteConfirm]", e);
    } finally {
      endSubmit();
    }
  }, [
    deleteWallpapers,
    isCollectionView,
    collectionId,
    onCollectionChanged,
    beginSubmit,
    endSubmit,
  ]);

  const handleAddToCollection = useCallback(
    async (wallpaperId: number, targetCollectionId: number) => {
      try {
        await addWallpapers(targetCollectionId, [wallpaperId]);
      } catch (e) {
        console.error("[addToCollection]", e);
      }
    },
    [],
  );

  return {
    manageMode,
    selectedIds,
    deleteDialogOpen,
    pendingDeleteIds,
    pendingRemovals,
    pendingDeletions,
    setDeleteDialogOpen,
    submitting,
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
