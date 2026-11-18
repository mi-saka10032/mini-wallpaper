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
import DropOverlay from "@/components/wallpaper/DropOverlay";
import VirtualGrid from "@/components/wallpaper/VirtualGrid";
import { WallpaperCard } from "@/components/wallpaper/WallpaperCard";
import { WallpaperCardContextMenuProvider } from "@/components/wallpaper/WallpaperCardContextMenu";
import { TagEditorDialog } from "@/components/wallpaper/TagEditorDialog";
import { SmartCollectionDialog } from "@/components/wallpaper/SmartCollectionDialog";
import { useDropImport } from "@/hooks/useDropImport";
import { useImportFeedback } from "@/hooks/useImportFeedback";
import { useManageMode } from "@/hooks/useManageMode";
import { useSortMode } from "@/hooks/useSortMode";
import { useWallpaperSearch } from "@/hooks/useWallpaperSearch";
import { cn } from "@/lib/utils";
import { useCollectionStore } from "@/stores/collectionStore";
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
  onPreview: (index: number) => void;
  onManageModeChange?: (active: boolean) => void;
}

const MainContent: React.FC<MainContentProps> = ({ className, onPreview, onManageModeChange }) => {
  const { t } = useTranslation();

  // 视图数据直接取自 store，不再由 App 自上而下灌入：
  // activeId 变化时 store 内部已完成拉取（含竞态守卫），这里只消费结果。
  const activeId = useWallpaperStore((s) => s.activeId);
  const wallpapers = useWallpaperStore((s) => s.viewWallpapers);

  const isCollectionView = activeId > 0;
  const collectionId = isCollectionView ? activeId : null;
  const isEmpty = wallpapers.length === 0;

  // 智能收藏夹成员由规则实时求值，不物化到 collection_wallpapers，
  // 因此手动拖拽排序无落库目标（reorder 会静默失败），需屏蔽排序入口；
  // 其「管理」入口也不再进入壁纸管理模式，而是直接编辑规则（见 smartDialogOpen）。
  const collections = useCollectionStore((s) => s.collections);
  const activeCollection = useMemo(
    () => (isCollectionView ? (collections.find((c) => c.id === activeId) ?? null) : null),
    [isCollectionView, collections, activeId],
  );
  const isSmartCollection = activeCollection?.kind === "smart";

  // ===== 独立 hooks 组合 =====
  const manage = useManageMode({
    isCollectionView,
    collectionId,
    onManageModeChange,
  });

  const sort = useSortMode({
    wallpapers,
    collectionId,
    onManageModeChange,
  });

  const search = useWallpaperSearch({ activeId });

  // 批量标签编辑对话框
  const [tagDialogOpen, setTagDialogOpen] = useState(false);

  // 智能收藏夹规则编辑对话框（替代智能收藏夹下的壁纸管理模式）
  const [smartDialogOpen, setSmartDialogOpen] = useState(false);

  // ===== 进入/退出模式时联动搜索重置 =====
  const enterManageMode = useCallback(() => {
    manage.enterManageMode();
    search.resetManageSearch();
    search.resetNormalSearch();
  }, [manage.enterManageMode, search.resetManageSearch, search.resetNormalSearch]);

  const exitManageMode = useCallback(async () => {
    const exited = await manage.exitManageMode();
    if (exited) search.resetManageSearch();
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
    sort.sortMode,
    sort.localOrder,
    manage.manageMode,
    manage.pendingRemovals,
    manage.pendingDeletions,
    wallpapers,
    isCollectionView,
    search.getFilteredWallpapers,
    search.getNormalFilteredWallpapers,
  ]);

  const wallpaperIds = useMemo(() => displayWallpapers.map((w) => w.id), [displayWallpapers]);

  // 用 ref 持有 wallpapers，避免 handleCardClick 依赖数组引用变化
  const wallpapersRef = useRef(wallpapers);
  wallpapersRef.current = wallpapers;

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
  const dropEnabled = !isCollectionView && !manage.manageMode && !sort.sortMode && !search.loading;

  // 两条拖拽通道共用同一套反馈（批量导入时 success 里的数量是唯一的进度参照）
  const { notify: notifyImport, notifyError } = useImportFeedback();

  const handleDropImport = useCallback(
    async (files: File[]) => {
      try {
        notifyImport(await useWallpaperStore.getState().importByFiles(files));
      } catch (e) {
        notifyError(e);
      }
    },
    [notifyImport, notifyError],
  );

  // Tauri 原生拖放：拿到路径，走与「点击导入」相同的后端链路
  const handleDropImportPaths = useCallback(
    async (paths: string[]) => {
      try {
        notifyImport(await useWallpaperStore.getState().importByPaths(paths));
      } catch (e) {
        notifyError(e);
      }
    },
    [notifyImport, notifyError],
  );

  const { isDragOver, dragHandlers } = useDropImport({
    enabled: dropEnabled,
    onImport: handleDropImport,
    onImportPaths: handleDropImportPaths,
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
      resetKey={activeId}
      forceDisable={false}
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
      <div className={cn("flex flex-1 flex-col overflow-hidden", className)}>
        {/* 操作栏 */}
        <div className="flex h-10 shrink-0 items-center gap-2 border-b border-border/50 px-4">
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
              pendingRemovalsCount={manage.pendingRemovals.length}
              pendingDeletionsCount={manage.pendingDeletions.length}
              onSelectAll={() => manage.selectAll(displayWallpapers)}
              onClearSelection={manage.clearSelection}
              onKeywordChange={search.setKeyword}
              onSortFieldChange={search.setSortField}
              onSortOrderToggle={() =>
                search.setSortOrder(search.sortOrder === "asc" ? "desc" : "asc")
              }
              onDeleteSelected={() => manage.handleDeleteRequest(Array.from(manage.selectedIds))}
              onManageTags={() => setTagDialogOpen(true)}
              onCancel={manage.cancelManageMode}
              onDone={exitManageMode}
              submitting={manage.submitting}
            />
          ) : (
            <NormalToolbar
              isCollectionView={isCollectionView}
              isSmartCollection={isSmartCollection}
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
              onEditSmartRule={() => setSmartDialogOpen(true)}
              onPickerConfirm={() => search.resetNormalSearch()}
            />
          )}
        </div>
        {/* 内容区。导入期间刻意不替换网格：VirtualGrid 一旦卸载，内部滚动容器
            的 scrollTop 会随 DOM 一起销毁，重新挂载后回顶；全屏蒙层由 App 叠加。 */}
        <div
          className={cn(
            "relative min-h-0 flex-1",
            isEmpty || displayWallpapers.length === 0 || sort.isDragEnabled
              ? "overflow-y-auto p-4"
              : "overflow-hidden",
          )}
          {...dragHandlers}
        >
          {isEmpty ? (
            <div className="flex h-full items-center justify-center">
              <div className="flex flex-col items-center gap-3 text-foreground/40">
                <ImagePlus className="size-12" strokeWidth={1} />
                <p className="text-sm">
                  {isCollectionView ? t("main.emptyCollection") : t("main.emptyAll")}
                </p>
              </div>
            </div>
          ) : displayWallpapers.length === 0 ? (
            <div className="flex h-full items-center justify-center">
              <div className="flex flex-col items-center gap-3 text-foreground/40">
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
              <AlertDialogTitle>
                {isCollectionView ? t("main.removeConfirmTitle") : t("main.deleteConfirmTitle")}
              </AlertDialogTitle>
              <AlertDialogDescription>
                {isCollectionView
                  ? t("main.removeConfirmDesc", { count: manage.pendingDeleteIds.length })
                  : t("main.deleteConfirmDesc", { count: manage.pendingDeleteIds.length })}
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel disabled={manage.submitting}>{t("main.cancel")}</AlertDialogCancel>
              <AlertDialogAction
                disabled={manage.submitting}
                onClick={(event) => {
                  event.preventDefault();
                  void manage.handleDeleteConfirm().catch(() => {});
                }}
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
        {/* 智能收藏夹规则编辑对话框（复用创建/编辑共用的构建器） */}
        {isSmartCollection && (
          <SmartCollectionDialog
            open={smartDialogOpen}
            onOpenChange={setSmartDialogOpen}
            editing={activeCollection}
          />
        )}{" "}
      </div>
    </WallpaperCardContextMenuProvider>
  );
};

export default MainContent;
