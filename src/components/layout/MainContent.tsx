import {
  DndContext,
  closestCenter,
  PointerSensor,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import type { DragEndEvent } from "@dnd-kit/core";
import {
  SortableContext,
  rectSortingStrategy,
} from "@dnd-kit/sortable";
import {
  GripVertical,
  ImagePlus,
  Plus,
  Search,
  Settings2,
  SortAsc,
  Trash2,
  Unlink,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useWallpaperStore } from "@/stores/wallpaperStore";
import type { Wallpaper } from "@/stores/wallpaperStore";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import WallpaperPickerDialog from "@/components/wallpaper/WallpaperPickerDialog";
import { sortWallpapers } from "@/components/wallpaper/WallpaperGrid";
import type { SortField, SortOrder } from "@/components/wallpaper/WallpaperGrid";
import ImportDropCard from "@/components/wallpaper/ImportDropCard";
import VirtualGrid from "@/components/wallpaper/VirtualGrid";
import { WallpaperCard, SortableWallpaperCard } from "@/components/wallpaper/WallpaperCard";
import { addWallpapers, removeWallpapers, reorderWallpapers } from "@/api/collectionWallpaper";

interface MainContentProps {
  activeId: number;
  wallpapers: Wallpaper[];
  onPreview: (index: number) => void;
  onCollectionChanged?: () => void;
  onManageModeChange?: (active: boolean) => void;
}

const MainContent: React.FC<MainContentProps> = ({
  activeId,
  wallpapers,
  onPreview,
  onCollectionChanged,
  onManageModeChange,
}) => {
  const { t } = useTranslation();
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

  // dnd-kit sensor: 需要拖动 10px 才触发，避免和点击选择冲突
  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: { distance: 10 },
    }),
  );

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

  const handleAddToCollection = useCallback(async (wallpaperId: number, collectionId: number) => {
    try {
      await addWallpapers(collectionId, [wallpaperId]);
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

  // 导入拖拽卡片：暂时隐藏
  const showImportCard = false;

  // 排序模式下的网格内容（dnd-kit 需要所有 DOM 在文档中，不能虚拟化）
  const sortableGridContent = (
    <div className="grid grid-cols-3 gap-3 xl:grid-cols-4 2xl:grid-cols-5">
      {displayWallpapers.map((wp, index) => (
        <SortableWallpaperCard
          key={wp.id}
          wallpaper={wp}
          index={index}
          activeId={activeId}
          manageMode={manageMode}
          selected={selectedIds.has(wp.id)}
          isCollectionView={isCollectionView}
          onClick={handleCardClick}
          onDelete={(id) => handleDeleteRequest([id])}
          onAddToCollection={handleAddToCollection}
        />
      ))}
    </div>
  );

  // 非排序模式下的网格内容（支持虚拟滚动）
  const virtualGridContent = (
    <VirtualGrid
      items={displayWallpapers}
      getKey={(wp) => wp.id}
      className="h-full p-4"
      forceDisable={false}
      trailingElement={showImportCard ? <ImportDropCard /> : undefined}
      renderItem={(wp, index) => (
        <WallpaperCard
          wallpaper={wp}
          index={index}
          activeId={activeId}
          manageMode={manageMode}
          selected={selectedIds.has(wp.id)}
          isCollectionView={isCollectionView}
          onClick={handleCardClick}
          onDelete={(id) => handleDeleteRequest([id])}
          onAddToCollection={handleAddToCollection}
        />
      )}
    />
  );

  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      {/* 操作栏 */}
      <div className="flex h-10 shrink-0 items-center gap-2 border-b border-border px-4">
        {sortMode ? (
          <>
            <GripVertical className="size-3.5 text-muted-foreground" />
            <span className="text-sm text-muted-foreground">{t("main.sortModeHint")}</span>
            {orderDirty && (
              <span className="ml-1 text-xs text-primary">{t("main.orderModified")}</span>
            )}
            <div className="flex-1" />
            <Button variant="ghost" size="sm" onClick={cancelSortMode}>
              {t("main.cancel")}
            </Button>
            <Button variant="outline" size="sm" onClick={exitSortMode} disabled={!orderDirty}>
              {t("main.save")}
            </Button>
          </>
        ) : manageMode ? (
          <>
            <span className="text-sm text-muted-foreground">{t("main.selected", { count: selectedIds.size })}</span>
            <button
              type="button"
              onClick={selectAll}
              className="text-sm text-primary hover:underline"
            >
              {t("main.selectAll")}
            </button>
            <button
              type="button"
              onClick={clearSelection}
              className="text-sm text-primary hover:underline"
            >
              {t("main.clearSelection")}
            </button>

            {/* 搜索框（管理模式） */}
            <div className="relative ml-2 max-w-44">
              <Search className="absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
              <Input
                value={keyword}
                onChange={(e) => setKeyword(e.target.value)}
                placeholder={t("grid.searchPlaceholder")}
                className="h-7 pl-7 pr-7 text-xs"
              />
              {keyword && (
                <button
                  type="button"
                  onClick={() => setKeyword("")}
                  className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded-sm p-0.5 text-muted-foreground hover:text-foreground"
                >
                  <X className="size-3" />
                </button>
              )}
            </div>

            {/* 前端排序（管理模式） */}
            <Select value={sortField} onValueChange={(v) => setSortField(v as typeof sortField)}>
              <SelectTrigger size="sm" className="h-7 w-auto gap-1 border-none bg-transparent px-2 text-xs text-muted-foreground shadow-none hover:bg-accent">
                <SortAsc className="size-3" />
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="created_at">{t("grid.sortByDate")}</SelectItem>
                <SelectItem value="name">{t("grid.sortByName")}</SelectItem>
                <SelectItem value="file_size">{t("grid.sortBySize")}</SelectItem>
                <SelectItem value="type">{t("grid.sortByType")}</SelectItem>
              </SelectContent>
            </Select>
            <button
              type="button"
              onClick={() => setSortOrder(sortOrder === "asc" ? "desc" : "asc")}
              className={cn(
                "flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground",
                sortOrder === "desc" && "rotate-180",
              )}
              title={sortOrder === "asc" ? t("grid.ascending") : t("grid.descending")}
            >
              <SortAsc className="size-3.5" />
            </button>

            <div className="flex-1" />
            {selectedIds.size > 0 && (
              <button
                type="button"
                onClick={() => handleDeleteRequest(Array.from(selectedIds))}
                className={cn(
                  "flex items-center gap-1 rounded-md px-2 py-1 text-sm transition-colors",
                  isCollectionView
                    ? "text-muted-foreground hover:bg-muted"
                    : "text-destructive hover:bg-destructive/10",
                )}
              >
                {isCollectionView ? (
                  <>
                    <Unlink className="size-3.5" />
                    {t("main.remove")}
                  </>
                ) : (
                  <>
                    <Trash2 className="size-3.5" />
                    {t("main.delete")}
                  </>
                )}
              </button>
            )}
            <Button variant="ghost" size="sm" onClick={cancelManageMode}>
              {t("main.cancel")}
            </Button>
            <Button variant="outline" size="sm" onClick={exitManageMode}>
              {t("main.done")}
            </Button>
          </>
        ) : (
          <>
            {/* 收藏夹视图：添加壁纸按钮 */}
            {isCollectionView && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setPickerOpen(true)}
                className="gap-1.5 text-muted-foreground"
              >
                <Plus className="size-3.5" />
                {t("main.addWallpaper")}
              </Button>
            )}

            <div className="flex-1" />

            {/* 常态搜索框（可折叠） */}
            {!isEmpty && (
              <div className="flex items-center">
                {searchExpanded ? (
                  <div className="relative animate-in fade-in slide-in-from-right-2 duration-200">
                    <Search className="absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
                    <Input
                      autoFocus
                      value={normalKeyword}
                      onChange={(e) => setNormalKeyword(e.target.value)}
                      placeholder={t("grid.searchPlaceholder")}
                      className="h-7 w-44 pl-7 pr-7 text-xs"
                      onBlur={() => {
                        if (!normalKeyword) setSearchExpanded(false);
                      }}
                      onKeyDown={(e) => {
                        if (e.key === "Escape") {
                          setNormalKeyword("");
                          setSearchExpanded(false);
                        }
                      }}
                    />
                    {normalKeyword && (
                      <button
                        type="button"
                        onClick={() => {
                          setNormalKeyword("");
                          setSearchExpanded(false);
                        }}
                        className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded-sm p-0.5 text-muted-foreground hover:text-foreground"
                      >
                        <X className="size-3" />
                      </button>
                    )}
                  </div>
                ) : (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => setSearchExpanded(true)}
                    className="gap-1.5 text-muted-foreground"
                  >
                    <Search className="size-3.5" />
                  </Button>
                )}
              </div>
            )}

            {/* 收藏夹视图：拖拽排序按钮 */}
            {!isEmpty && isCollectionView && (
              <Button
                variant="ghost"
                size="sm"
                onClick={enterSortMode}
                className="gap-1.5 text-muted-foreground"
              >
                <GripVertical className="size-3.5" />
                {t("main.sortWallpapers")}
              </Button>
            )}

            {!isEmpty && (
              <Button
                variant="ghost"
                size="sm"
                onClick={enterManageMode}
                className="gap-1.5 text-muted-foreground"
              >
                <Settings2 className="size-3.5" />
                {t("main.manageWallpapers")}
              </Button>
            )}
          </>
        )}
      </div>

      {/* 内容区 */}
      <div className={cn(
        "flex-1 overflow-hidden",
        (loading || isEmpty || displayWallpapers.length === 0 || isDragEnabled) && "overflow-y-auto p-4",
      )}>
        {loading ? (
          <div className="flex h-full items-center justify-center">
            <p className="text-sm text-muted-foreground">{t("main.importing")}</p>
          </div>
        ) : isEmpty ? (
          <div className="flex h-full items-center justify-center">
            <div className="flex flex-col items-center gap-3 text-muted-foreground/60">
              <ImagePlus className="size-12" strokeWidth={1} />
              <p className="text-sm">
                {isCollectionView ? t("main.emptyCollection") : t("main.emptyAll")}
              </p>
            </div>
          </div>
        ) : displayWallpapers.length === 0 ? (
          <div className="flex h-full items-center justify-center">
            <div className="flex flex-col items-center gap-3 text-muted-foreground/60">
              <Search className="size-10" strokeWidth={1} />
              <p className="text-sm">{t("grid.noResults")}</p>
            </div>
          </div>
        ) : isDragEnabled ? (
          <DndContext
            sensors={sensors}
            collisionDetection={closestCenter}
            onDragEnd={handleDragEnd}
          >
            <SortableContext items={wallpaperIds} strategy={rectSortingStrategy}>
              {sortableGridContent}
            </SortableContext>
          </DndContext>
        ) : (
          virtualGridContent
        )}
      </div>

      {/* 底部状态栏 */}
      <div className="flex h-8 shrink-0 items-center border-t border-border px-4">
        <span className="text-xs text-muted-foreground">
          {manageMode && selectedIds.size > 0
            ? t("main.selectedTotal", { selected: selectedIds.size, total: displayWallpapers.length })
            : t("main.total", { count: displayWallpapers.length })}
        </span>
        {manageMode && keyword && displayWallpapers.length !== wallpapers.length && (
          <span className="ml-2 text-xs text-muted-foreground">
            {t("grid.filterResult", { filtered: displayWallpapers.length, total: wallpapers.length })}
          </span>
        )}
        {!manageMode && !sortMode && normalKeyword && displayWallpapers.length !== wallpapers.length && (
          <span className="ml-2 text-xs text-muted-foreground">
            {t("grid.filterResult", { filtered: displayWallpapers.length, total: wallpapers.length })}
          </span>
        )}
        {manageMode && pendingRemovals.length > 0 && (
          <span className="ml-2 text-xs text-orange-500">
            {t("main.pendingRemovals", { count: pendingRemovals.length })}
          </span>
        )}
        {manageMode && pendingDeletions.length > 0 && (
          <span className="ml-2 text-xs text-orange-500">
            {t("main.pendingDeletions", { count: pendingDeletions.length })}
          </span>
        )}
      </div>

      {/* 删除确认 */}
      <AlertDialog open={deleteDialogOpen} onOpenChange={setDeleteDialogOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{isCollectionView ? t("main.removeConfirmTitle") : t("main.deleteConfirmTitle")}</AlertDialogTitle>
            <AlertDialogDescription>
              {isCollectionView
                ? t("main.removeConfirmDesc", { count: pendingDeleteIds.length })
                : t("main.deleteConfirmDesc", { count: pendingDeleteIds.length })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("main.cancel")}</AlertDialogCancel>
            <AlertDialogAction
              onClick={handleDeleteConfirm}
              className={
                isCollectionView
                  ? ""
                  : "bg-destructive text-destructive-foreground hover:bg-destructive/90"
              }
            >
              {isCollectionView ? t("main.remove") : t("main.delete")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* 壁纸选择器 Dialog */}
      {isCollectionView && collectionId !== null && (
        <WallpaperPickerDialog
          open={pickerOpen}
          collectionId={collectionId}
          existingWallpaperIds={new Set(wallpapers.map((w) => w.id))}
          onClose={() => setPickerOpen(false)}
          onConfirm={handlePickerConfirm}
        />
      )}
    </div>
  );
};

export default MainContent;
