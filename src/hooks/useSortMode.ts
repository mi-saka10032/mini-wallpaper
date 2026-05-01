import { useCallback, useState } from "react";
import type { DragEndEvent } from "@dnd-kit/core";
import type { Wallpaper } from "@/stores/wallpaperStore";
import { reorderWallpapers } from "@/api/collectionWallpaper";

export interface UseSortModeOptions {
  wallpapers: Wallpaper[];
  collectionId: number | null;
  onCollectionChanged?: () => void;
  onManageModeChange?: (active: boolean) => void;
}

export function useSortMode({
  wallpapers,
  collectionId,
  onCollectionChanged,
  onManageModeChange,
}: UseSortModeOptions) {
  const [sortMode, setSortMode] = useState(false);
  const [localOrder, setLocalOrder] = useState<Wallpaper[] | null>(null);
  const [orderDirty, setOrderDirty] = useState(false);

  const enterSortMode = useCallback(() => {
    setSortMode(true);
    setLocalOrder([...wallpapers]);
    setOrderDirty(false);
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
    sortMode,
    localOrder,
    orderDirty,
    isDragEnabled,
    enterSortMode,
    exitSortMode,
    cancelSortMode,
    handleDragEnd,
  };
}
