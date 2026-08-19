import type { Wallpaper } from "@/api/config";
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
import { toast } from "@/components/ui/toast";
import DropOverlay from "@/components/wallpaper/DropOverlay";
import VirtualGrid from "@/components/wallpaper/VirtualGrid";
import { WallpaperCard } from "@/components/wallpaper/WallpaperCard";
import { WallpaperCardContextMenuProvider } from "@/components/wallpaper/WallpaperCardContextMenu";
import { TagEditorDialog } from "@/components/wallpaper/TagEditorDialog";
import { useDropImport } from "@/hooks/useDropImport";
import { useManageMode } from "@/hooks/useManageMode";
import { useSortMode } from "@/hooks/useSortMode";
import { useWallpaperSearch } from "@/hooks/useWallpaperSearch";
import { cn } from "@/lib/utils";
import { useWallpaperStore } from "@/stores/wallpaperStore";
import { ImagePlus, Search } from "lucide-react";
import { lazy, Suspense, useCallback, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import ManageToolbar from "./ManageToolbar";
import NormalToolbar from "./NormalToolbar";
import SortToolbar from "./SortToolbar";
import StatusBar from "./StatusBar";

// 排序网格组件懒加载（包含 @dnd-kit 依赖）
const SortableGrid = lazy(() => import("@/components/wallpaper/SortableGrid"));

interface MainContentProps {
  className?: string;
  activeId: number;
  wallpapers: Wallpaper[];
  onPreview: (index: number) => void;
  onCollectionChanged?: () => void;
  onManageModeChange?: (active: boolean) => void;
}

const MainContent: React.FC<MainContentProps> = ({
  className,
  activeId,
  wallpapers,
  onPreview,
  onCollectionChanged,
  onManageModeChange,
}) => {
  const { t } = useTranslation();

  const isCollectionView = activeId > 0;
  const collectionId = isCollectionView ? activeId : null;
  const isEmpty = wallpapers.length === 0;

  // ===== 独立 hooks 组合 =====
  const manage = useManageMode({
    isCollectionView,
    collectionId,
    onCollectionChanged,
    onManageModeChange,
  });

  const sort = useSortMode({
    wallpapers,
    collectionId,
    onCollectionChanged,
    onManageModeChange,
  });

  const search = useWallpaperSearch({ activeId });

  // 批量标签编辑对话框
  const [tagDialogOpen, setTagDialogOpen] = useState(false);

  // ===== 进入/退出模式时联动搜索重置 =====
  const enterManageMode = useCallback(() => {
    manage.enterManageMode();
    search.resetManageSearch();
    search.resetNormalSearch();
  }, [manage.enterManageMode, search.resetManageSearch, search.resetNormalSearch]);

  const exitManageMode = useCallback(async () => {
    await manage.exitManageMode();
    search.resetManageSearch();
  }, [manage.exitManageMode, search.resetManageSearch]);

  const enterSortMode = useCallback(() => {
    sort.enterSortMode();
    search.resetNormalSearch();
  }, [sort.enterSortMode, search.resetNormalSearch]);

  // ===== displayWallpapers 计算（组件层交叉逻辑） =====
  const displayWallpapers = useMemo(() => {
    // 排序模式：使用 localOrder
    if (sort.sortMode && sort.localOrder) {
      return sort.localOrder;
    }
    // 管理模式：过滤 pending + 搜索排序
    if (manage.manageMode) {
      let source = wallpapers;
      if (isCollectionView && manage.pendingRemovals.length > 0) {
        source = source.filter((w) => !manage.pendingRemovals.includes(w.id));
      }
      if (!isCollectionView && manage.pendingDeletions.length > 0) {
        source = source.filter((w) => !manage.pendingDeletions.includes(w.id));
      }
      return search.getFilteredWallpapers(source);
    }
    // 常态模式：normalKeyword 过滤
    return search.getNormalFilteredWallpapers(wallpapers);
  }, [
    sort.sortMode, sort.localOrder,
    manage.manageMode, manage.pendingRemovals, manage.pendingDeletions,
    wallpapers, isCollectionView,
    search.getFilteredWallpapers, search.getNormalFilteredWallpapers,
  ]);

  const wallpaperIds = useMemo(() => displayWallpapers.map((w) => w.id), [displayWallpapers]);

  // 用 ref 持有 wallpapers，避免 handleCardClick 依赖数组引用变化
  const wallpapersRef = useRef(wallpapers);
  wallpapersRef.current = wallpapers;

  // renderVersion：selectedIds 变化时递增，驱动 VirtualGrid 虚拟行重渲染
  const renderVersionRef = useRef(0);
  const prevSelectedIdsRef = useRef(manage.selectedIds);
  if (prevSelectedIdsRef.current !== manage.selectedIds) {
    prevSelectedIdsRef.current = manage.selectedIds;
    renderVersionRef.current += 1;
  }
  const renderVersion = renderVersionRef.current;

  // ===== 稳定的单个删除回调（避免内联函数导致 WallpaperCard memo 失效） =====
  const handleSingleDelete = useCallback(
    (id: number) => manage.handleDeleteRequest([id]),
    [manage.handleDeleteRequest],
  );

  // ===== 卡片点击 =====
  const handleCardClick = useCallback(
    (wp: Wallpaper, _index: number, _e: React.MouseEvent) => {
      if (sort.sortMode) return;
      if (manage.manageMode) {
        manage.toggleSelect(wp.id);
      } else {
        const realIndex = wallpapersRef.current.findIndex((w) => w.id === wp.id);
        onPreview(realIndex !== -1 ? realIndex : _index);
      }
    },
    [sort.sortMode, manage.manageMode, manage.toggleSelect, onPreview],
  );

  // ===== 拖拽导入（H5 drag）=====
  // 仅本地壁纸普通模式生效：非收藏夹 + 非管理模式 + 非排序模式 + 非加载中
  const dropEnabled =
    !isCollectionView && !manage.manageMode && !sort.sortMode && !search.loading;

  const handleDropImport = useCallback(
    async (files: File[]) => {
      const result = await useWallpaperStore.getState().importByFiles(files);
      if (result.imported > 0) {
        toast.success(t("main.importedCount", { count: result.imported }));
      }
      if (result.skipped > 0) {
        toast.info(t("main.skippedUnsupported", { count: result.skipped }));
      }
      if (result.rejectedBySize > 0) {
        toast.warning(t("main.skippedBySize", { count: result.rejectedBySize }));
      }
    },
    [t],
  );

  const { isDragOver, dragHandlers } = useDropImport({
    enabled: dropEnabled,
    onImport: handleDropImport,
  });

  // 排序模式下的网格内容（懒加载 SortableGrid，包含 @dnd-kit）
  const sortableGridContent = (
    <Suspense fallback={null}>
      <SortableGrid
        wallpapers={displayWallpapers}
        wallpaperIds={wallpaperIds}
        activeId={activeId}
        manageMode={manage.manageMode}
        selectedIds={manage.selectedIds}
        isCollectionView={isCollectionView}
        onDragEnd={sort.handleDragEnd}
        onClick={handleCardClick}
        onDelete={handleSingleDelete}
        onAddToCollection={manage.handleAddToCollection}
      />
    </Suspense>
  );

  // 非排序模式下的网格内容（支持虚拟滚动）
  const virtualGridContent = (
    <VirtualGrid
      items={displayWallpapers}
      getKey={(wp) => wp.id}
      className="h-full p-4"
      forceDisable={false}
      renderVersion={renderVersion}
      renderItem={(wp, index) => (
        <WallpaperCard
          wallpaper={wp}
          index={index}
          activeId={activeId}
          manageMode={manage.manageMode}
          selected={manage.selectedIds.has(wp.id)}
          isCollectionView={isCollectionView}
          onClick={handleCardClick}
          onDelete={handleSingleDelete}
          onAddToCollection={manage.handleAddToCollection}
        />
      )}
    />
  );

  return (
    <WallpaperCardContextMenuProvider>
      <div className={cn('flex flex-1 flex-col overflow-hidden', className)}>
        {/* 操作栏 */}
        <div className="flex h-10 shrink-0 items-center gap-2 border-b border-border/40 px-4">
          {sort.sortMode ? (
            <SortToolbar
              orderDirty={sort.orderDirty}
              onCancel={sort.cancelSortMode}
              onSave={sort.exitSortMode}
            />
          ) : manage.manageMode ? (
            <ManageToolbar
              selectedCount={manage.selectedIds.size}
              keyword={search.keyword}
              sortField={search.sortField}
              sortOrder={search.sortOrder}
              isCollectionView={isCollectionView}
              onSelectAll={() => manage.selectAll(displayWallpapers)}
              onClearSelection={manage.clearSelection}
              onKeywordChange={search.setKeyword}
              onSortFieldChange={search.setSortField}
              onSortOrderToggle={() => search.setSortOrder(search.sortOrder === "asc" ? "desc" : "asc")}
              onDeleteSelected={() => manage.handleDeleteRequest(Array.from(manage.selectedIds))}
              onManageTags={() => setTagDialogOpen(true)}
              onCancel={manage.cancelManageMode}
              onDone={exitManageMode}
            />
          ) : (
            <NormalToolbar
              isCollectionView={isCollectionView}
              isEmpty={isEmpty}
              searchExpanded={search.searchExpanded}
              normalKeyword={search.normalKeyword}
              collectionId={collectionId}
              collectionWallpapers={wallpapers}
              onRefresh={() => useWallpaperStore.getState().fetchWallpapers()}
              onSearchExpand={() => search.setSearchExpanded(true)}
              onSearchCollapse={() => search.setSearchExpanded(false)}
              onNormalKeywordChange={search.setNormalKeyword}
              onEnterSortMode={enterSortMode}
              onEnterManageMode={enterManageMode}
              onPickerConfirm={() => {
                search.resetNormalSearch();
                onCollectionChanged?.();
              }}
            />
          )}
        </div>

        {/* 内容区 */}
        <div
          className={cn(
            "relative min-h-0 flex-1",
            (search.loading || isEmpty || displayWallpapers.length === 0 || sort.isDragEnabled)
              ? "overflow-y-auto p-4"
              : "overflow-hidden",
          )}
          {...dragHandlers}
        >
          {search.loading ? (
            <div className="flex h-full items-center justify-center">
              <p className="text-sm text-foreground/50">{t("main.importing")}</p>
            </div>
          ) : isEmpty ? (
            <div className="flex h-full items-center justify-center">
              <div className="flex flex-col items-center gap-3 text-foreground/30">
                <ImagePlus className="size-12" strokeWidth={1} />
                <p className="text-sm">
                  {isCollectionView ? t("main.emptyCollection") : t("main.emptyAll")}
                </p>
              </div>
            </div>
          ) : displayWallpapers.length === 0 ? (
            <div className="flex h-full items-center justify-center">
              <div className="flex flex-col items-center gap-3 text-foreground/30">
                <Search className="size-10" strokeWidth={1} />
                <p className="text-sm">{t("grid.noResults")}</p>
              </div>
            </div>
          ) : sort.isDragEnabled ? (
            sortableGridContent
          ) : (
            virtualGridContent
          )}

          {/* 拖拽导入蒙层（仅启用时根据 isDragOver 显示） */}
          {dropEnabled && <DropOverlay visible={isDragOver} />}
        </div>

        {/* 底部状态栏 */}
        <StatusBar
          manageMode={manage.manageMode}
          sortMode={sort.sortMode}
          selectedCount={manage.selectedIds.size}
          displayCount={displayWallpapers.length}
          totalCount={wallpapers.length}
          keyword={search.keyword}
          normalKeyword={search.normalKeyword}
          pendingRemovalsCount={manage.pendingRemovals.length}
          pendingDeletionsCount={manage.pendingDeletions.length}
        />

        {/* 删除确认 */}
        <AlertDialog open={manage.deleteDialogOpen} onOpenChange={manage.setDeleteDialogOpen}>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>{isCollectionView ? t("main.removeConfirmTitle") : t("main.deleteConfirmTitle")}</AlertDialogTitle>
              <AlertDialogDescription>
                {isCollectionView
                  ? t("main.removeConfirmDesc", { count: manage.pendingDeleteIds.length })
                  : t("main.deleteConfirmDesc", { count: manage.pendingDeleteIds.length })}
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>{t("main.cancel")}</AlertDialogCancel>
              <AlertDialogAction
                onClick={manage.handleDeleteConfirm}
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

        {/* 批量标签编辑对话框 */}
        <TagEditorDialog
          open={tagDialogOpen}
          onOpenChange={setTagDialogOpen}
          mode="batch"
          wallpaperIds={Array.from(manage.selectedIds)}
        />
      </div>
    </WallpaperCardContextMenuProvider>
  );
};

export default MainContent;