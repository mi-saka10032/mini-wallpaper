import {
  DndContext,
  closestCenter,
  PointerSensor,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  SortableContext,
  rectSortingStrategy,
} from "@dnd-kit/sortable";
import { ImagePlus, Search } from "lucide-react";
import { useTranslation } from "react-i18next";
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
import { cn } from "@/lib/utils";
import WallpaperPickerDialog from "@/components/wallpaper/WallpaperPickerDialog";
import ImportDropCard from "@/components/wallpaper/ImportDropCard";
import VirtualGrid from "@/components/wallpaper/VirtualGrid";
import { WallpaperCard, SortableWallpaperCard } from "@/components/wallpaper/WallpaperCard";
import { useMainContent } from "@/hooks/useMainContent";
import SortToolbar from "./SortToolbar";
import ManageToolbar from "./ManageToolbar";
import NormalToolbar from "./NormalToolbar";
import StatusBar from "./StatusBar";

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

  const {
    loading,
    manageMode,
    sortMode,
    selectedIds,
    deleteDialogOpen,
    pendingDeleteIds,
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
    setDeleteDialogOpen,
    setPickerOpen,
    setKeyword,
    setSortField,
    setSortOrder,
    setNormalKeyword,
    setSearchExpanded,
    enterManageMode,
    exitManageMode,
    cancelManageMode,
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
  } = useMainContent({
    activeId,
    wallpapers,
    onPreview,
    onCollectionChanged,
    onManageModeChange,
  });

  // dnd-kit sensor: 需要拖动 10px 才触发，避免和点击选择冲突
  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: { distance: 10 },
    }),
  );

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
          <SortToolbar
            orderDirty={orderDirty}
            onCancel={cancelSortMode}
            onSave={exitSortMode}
          />
        ) : manageMode ? (
          <ManageToolbar
            selectedCount={selectedIds.size}
            keyword={keyword}
            sortField={sortField}
            sortOrder={sortOrder}
            isCollectionView={isCollectionView}
            onSelectAll={selectAll}
            onClearSelection={clearSelection}
            onKeywordChange={setKeyword}
            onSortFieldChange={setSortField}
            onSortOrderToggle={() => setSortOrder(sortOrder === "asc" ? "desc" : "asc")}
            onDeleteSelected={() => handleDeleteRequest(Array.from(selectedIds))}
            onCancel={cancelManageMode}
            onDone={exitManageMode}
          />
        ) : (
          <NormalToolbar
            isCollectionView={isCollectionView}
            isEmpty={isEmpty}
            searchExpanded={searchExpanded}
            normalKeyword={normalKeyword}
            onOpenPicker={() => setPickerOpen(true)}
            onSearchExpand={() => setSearchExpanded(true)}
            onSearchCollapse={() => setSearchExpanded(false)}
            onNormalKeywordChange={setNormalKeyword}
            onEnterSortMode={enterSortMode}
            onEnterManageMode={enterManageMode}
          />
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
      <StatusBar
        manageMode={manageMode}
        sortMode={sortMode}
        selectedCount={selectedIds.size}
        displayCount={displayWallpapers.length}
        totalCount={wallpapers.length}
        keyword={keyword}
        normalKeyword={normalKeyword}
        pendingRemovalsCount={pendingRemovals.length}
        pendingDeletionsCount={pendingDeletions.length}
      />

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
