import { convertFileSrc } from "@tauri-apps/api/core";
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
  useSortable,
  rectSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import {
  Check,
  Film,
  FolderPlus,
  GripVertical,
  Image,
  ImagePlus,
  Monitor,
  Plus,
  Search,
  Settings2,
  SortAsc,
  Star,
  Trash2,
  Unlink,
  X,
} from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useWallpaperStore } from "@/stores/wallpaperStore";
import type { Wallpaper } from "@/stores/wallpaperStore";
import { useCollectionStore } from "@/stores/collectionStore";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
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
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import WallpaperPickerDialog from "@/components/wallpaper/WallpaperPickerDialog";
import ImportDropCard from "@/components/wallpaper/ImportDropCard";
import { addWallpapers, removeWallpapers, reorderWallpapers } from "@/api/collectionWallpaper";
import { useMonitorConfigStore } from "@/stores/monitorConfigStore";
import { useSettingStore, SETTING_KEYS } from "@/stores/settingStore";
import { getWallpapers as getCollectionWallpapers } from "@/api/collection";

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

  // 拖拽排序：本地排序状态（仅收藏夹管理模式下使用）
  const [localOrder, setLocalOrder] = useState<Wallpaper[] | null>(null);
  const [orderDirty, setOrderDirty] = useState(false);

  // 添加壁纸到收藏夹的 picker
  const [pickerOpen, setPickerOpen] = useState(false);

  // 搜索 + 排序状态
  const [keyword, setKeyword] = useState("");
  const [sortField, setSortField] = useState<"name" | "created_at" | "file_size" | "type">("created_at");
  const [sortOrder, setSortOrder] = useState<"asc" | "desc">("desc");

  const isCollectionView = activeId > 0;
  const collectionId = isCollectionView ? activeId : null;
  const isEmpty = wallpapers.length === 0;

  // 搜索 + 排序后的壁纸列表（非管理模式下使用）
  const filteredWallpapers = useMemo(() => {
    let result = wallpapers;
    // 关键词过滤
    if (keyword.trim()) {
      const kw = keyword.trim().toLowerCase();
      result = result.filter((w) => w.name.toLowerCase().includes(kw));
    }
    // 排序
    const sorted = [...result].sort((a, b) => {
      let cmp = 0;
      switch (sortField) {
        case "name":
          cmp = a.name.localeCompare(b.name, undefined, { numeric: true });
          break;
        case "created_at":
          cmp = a.created_at.localeCompare(b.created_at);
          break;
        case "file_size":
          cmp = (a.file_size ?? 0) - (b.file_size ?? 0);
          break;
        case "type":
          cmp = a.type.localeCompare(b.type);
          break;
      }
      return sortOrder === "asc" ? cmp : -cmp;
    });
    return sorted;
  }, [wallpapers, keyword, sortField, sortOrder]);

  // 拖拽排序时使用本地排序列表，否则用过滤排序后的列表
  const displayWallpapers = (manageMode && isCollectionView && localOrder) ? localOrder : filteredWallpapers;
  const wallpaperIds = useMemo(() => displayWallpapers.map((w) => w.id), [displayWallpapers]);

  // dnd-kit sensor: 需要拖动 10px 才触发，避免和点击选择冲突
  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: { distance: 10 },
    }),
  );

  const enterManageMode = useCallback(() => {
    setManageMode(true);
    setSelectedIds(new Set());
    onManageModeChange?.(true);
    // 收藏夹模式进入管理时，初始化本地排序副本
    if (activeId > 0) {
      setLocalOrder([...wallpapers]);
      setOrderDirty(false);
    }
  }, [activeId, wallpapers, onManageModeChange]);

  const exitManageMode = useCallback(async () => {
    // 退出管理模式时，如果排序有变更，持久化到后端
    if (isCollectionView && collectionId !== null && orderDirty && localOrder) {
      try {
        await reorderWallpapers(collectionId, localOrder.map((w) => w.id));
        onCollectionChanged?.();
      } catch (e) {
        console.error("[reorderWallpapers]", e);
      }
    }
    setManageMode(false);
    setSelectedIds(new Set());
    setLocalOrder(null);
    setOrderDirty(false);
    onManageModeChange?.(false);
  }, [isCollectionView, collectionId, orderDirty, localOrder, onCollectionChanged, onManageModeChange]);

  const cancelManageMode = useCallback(() => {
    setManageMode(false);
    setSelectedIds(new Set());
    setLocalOrder(null);
    setOrderDirty(false);
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
    setPendingDeleteIds(ids);
    setDeleteDialogOpen(true);
  }, []);

  const handleDeleteConfirm = useCallback(async () => {
    if (isCollectionView && collectionId !== null) {
      // 收藏夹视图：解除关联
      await removeWallpapers(collectionId!, pendingDeleteIds);
      // 同步更新本地排序列表
      if (localOrder) {
        setLocalOrder(localOrder.filter((w) => !pendingDeleteIds.includes(w.id)));
      }
      onCollectionChanged?.();
    } else {
      // 全部壁纸视图：彻底删除文件
      await deleteWallpapers(pendingDeleteIds);
    }
    setPendingDeleteIds([]);
    setDeleteDialogOpen(false);
    setSelectedIds(new Set());
  }, [deleteWallpapers, pendingDeleteIds, isCollectionView, collectionId, onCollectionChanged, localOrder]);

  const handleCardClick = useCallback(
    (wp: Wallpaper, index: number, _e: React.MouseEvent) => {
      if (manageMode) {
        // 管理模式下点击即 toggle 选中状态（多选）
        toggleSelect(wp.id);
      } else {
        onPreview(index);
      }
    },
    [manageMode, toggleSelect, onPreview],
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
    onCollectionChanged?.();
  }, [onCollectionChanged]);

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

  // 是否启用拖拽排序（收藏夹 + 管理模式）
  const isDragEnabled = manageMode && isCollectionView;

  // 导入拖拽卡片：暂时隐藏，等待 Tauri 拖拽事件在 Win11 下的兼容性修复后再启用
  // 原始条件：activeId === 0 && !manageMode
  const showImportCard = false;

  const gridContent = (
    <div className="grid grid-cols-3 gap-3 xl:grid-cols-4 2xl:grid-cols-5">
      {displayWallpapers.map((wp, index) =>
        isDragEnabled ? (
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
        ) : (
          <WallpaperCard
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
        ),
      )}
      {showImportCard && <ImportDropCard />}
    </div>
  );

  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      {/* 操作栏 */}
      <div className="flex h-10 shrink-0 items-center gap-2 border-b border-border px-4">
        {manageMode ? (
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

            {/* 搜索框 */}
            {!isEmpty && (
              <div className="relative max-w-48">
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
            )}

            {/* 排序 */}
            {!isEmpty && (
              <>
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
              </>
            )}

            <div className="flex-1" />

            {/* 筛选结果提示 */}
            {keyword && filteredWallpapers.length !== wallpapers.length && (
              <span className="text-xs text-muted-foreground">
                {t("grid.filterResult", { filtered: filteredWallpapers.length, total: wallpapers.length })}
              </span>
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
      <div className="flex-1 overflow-y-auto p-4">
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
              {gridContent}
            </SortableContext>
          </DndContext>
        ) : (
          gridContent
        )}
      </div>

      {/* 底部状态栏 */}
      <div className="flex h-8 shrink-0 items-center border-t border-border px-4">
        <span className="text-xs text-muted-foreground">
          {manageMode && selectedIds.size > 0
            ? t("main.selectedTotal", { selected: selectedIds.size, total: displayWallpapers.length })
            : t("main.total", { count: displayWallpapers.length })}
        </span>
        {isDragEnabled && orderDirty && (
          <span className="ml-2 text-xs text-primary">{t("main.orderModified")}</span>
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

/** 壁纸卡片 Props */
interface WallpaperCardProps {
  wallpaper: Wallpaper;
  index: number;
  activeId: number;
  manageMode: boolean;
  selected: boolean;
  isCollectionView: boolean;
  isDragging?: boolean;
  dragHandleProps?: React.HTMLAttributes<HTMLDivElement>;
  onClick: (wp: Wallpaper, index: number, e: React.MouseEvent) => void;
  onDelete: (id: number) => void;
  onAddToCollection: (wallpaperId: number, collectionId: number) => void;
}

/** 单个壁纸卡片（内容渲染） */
const WallpaperCardContent: React.FC<WallpaperCardProps & { style?: React.CSSProperties }> = ({
  wallpaper,
  index,
  activeId,
  manageMode,
  selected,
  isCollectionView,
  isDragging = false,
  dragHandleProps,
  onClick,
  onDelete,
  onAddToCollection,
  style,
}) => {
  const collections = useCollectionStore((s) => s.collections);
  const configs = useMonitorConfigStore((s) => s.configs);
  const upsert = useMonitorConfigStore((s) => s.upsert);
  const upsertAll = useMonitorConfigStore((s) => s.upsertAll);
  const displayMode = useSettingStore((s) => s.settings[SETTING_KEYS.DISPLAY_MODE] ?? "independent");
  const { t } = useTranslation();
  const TypeIcon = wallpaper.type === "video" ? Film : Image;

  // 有效（active）的显示器配置列表
  const activeConfigs = useMemo(() => configs.filter((c) => c.active), [configs]);
  const isSyncMode = displayMode === "mirror" || displayMode === "extend";

  /**
   * 处理"设置为 X 显示器的壁纸"
   * 根据 activeId（0=本地壁纸栏, >0=收藏夹栏）和目标显示器的 config 状态，分 6 种场景处理
   */
  const handleSetAsWallpaper = useCallback(
    async (monitorId: string) => {
      const targetConfig = configs.find((c) => c.monitor_id === monitorId);
      const wallpaperId = wallpaper.id;
      const collectionId = activeId > 0 ? activeId : null; // 当前所在收藏夹 id

      if (activeId === 0) {
        // ===== 本地壁纸栏（activeId === 0）=====
        if (!targetConfig?.collection_id) {
          // 场景 1a：显示器无收藏夹，直接更新壁纸 id
          if (isSyncMode) {
            await upsertAll({ wallpaperId });
          } else {
            await upsert({ monitorId, wallpaperId });
          }
        } else {
          // 显示器有收藏夹，需查询壁纸是否在该收藏夹中
          try {
            const wallpapersInCollection = await getCollectionWallpapers(targetConfig.collection_id);
            const isInCollection = wallpapersInCollection.some((w) => w.id === wallpaperId);

            if (isInCollection) {
              // 场景 1b：壁纸在收藏夹中，直接更新壁纸 id
              if (isSyncMode) {
                await upsertAll({ wallpaperId });
              } else {
                await upsert({ monitorId, wallpaperId });
              }
            } else {
              // 场景 1c：壁纸不在收藏夹中，强制切换至单张壁纸播放
              if (isSyncMode) {
                await upsertAll({ wallpaperId, clearCollection: true, isEnabled: false });
              } else {
                await upsert({ monitorId, wallpaperId, clearCollection: true, isEnabled: false });
              }
            }
          } catch (e) {
            console.error("[handleSetAsWallpaper] query collection wallpapers failed:", e);
            // 查询失败时保守处理：强制切换至单张
            if (isSyncMode) {
              await upsertAll({ wallpaperId, clearCollection: true, isEnabled: false });
            } else {
              await upsert({ monitorId, wallpaperId, clearCollection: true, isEnabled: false });
            }
          }
        }
      } else {
        // ===== 收藏夹栏（activeId > 0）=====
        if (!targetConfig?.collection_id) {
          // 场景 2a：显示器无收藏夹，设置收藏夹播放（默认不启用轮播）
          if (isSyncMode) {
            await upsertAll({ wallpaperId, collectionId });
          } else {
            await upsert({ monitorId, wallpaperId, collectionId });
          }
        } else if (targetConfig.collection_id === collectionId) {
          // 场景 2b：同一收藏夹，切换壁纸 id
          if (isSyncMode) {
            await upsertAll({ wallpaperId });
          } else {
            await upsert({ monitorId, wallpaperId });
          }
        } else {
          // 场景 2c：不同收藏夹，切换收藏夹播放
          if (isSyncMode) {
            await upsertAll({ wallpaperId, collectionId });
          } else {
            await upsert({ monitorId, wallpaperId, collectionId });
          }
        }
      }
    },
    [wallpaper.id, activeId, configs, isSyncMode, upsert, upsertAll],
  );

  const card = (
    <div
      className={cn(
        "group relative cursor-pointer overflow-hidden rounded-lg border bg-muted/30 transition-all",
        manageMode && selected
          ? "border-primary ring-2 ring-primary"
          : "border-border hover:ring-2 hover:ring-primary/50",
        isDragging && "opacity-50 shadow-lg ring-2 ring-primary",
      )}
      style={style}
      onClick={(e) => {
        if (!isDragging) onClick(wallpaper, index, e);
      }}
    >
      {/* 拖拽手柄（收藏夹管理模式），阻止点击冒泡以避免触发选中 */}
      {manageMode && isCollectionView && dragHandleProps && (
        <div
          {...dragHandleProps}
          onClick={(e) => e.stopPropagation()}
          className="absolute right-1.5 bottom-8 z-20 flex size-6 cursor-grab items-center justify-center rounded bg-black/40 text-white opacity-0 transition-opacity active:cursor-grabbing group-hover:opacity-100"
        >
          <GripVertical className="size-3.5" />
        </div>
      )}

      {manageMode && selected && (
        <div className="absolute left-1.5 top-1.5 z-10 flex size-5 items-center justify-center rounded-full bg-primary text-primary-foreground">
          <Check className="size-3" />
        </div>
      )}

      {manageMode && !selected && (
        <div className="absolute left-1.5 top-1.5 z-10 flex size-5 items-center justify-center rounded-full border-2 border-white/60 bg-black/20 opacity-0 transition-opacity group-hover:opacity-100" />
      )}

      <div className="aspect-video bg-muted">
        {wallpaper.thumb_path ? (
          <img
            src={convertFileSrc(wallpaper.thumb_path)}
            alt={wallpaper.name}
            className="size-full object-cover"
            loading="lazy"
          />
        ) : (
          <div className="flex size-full items-center justify-center">
            <TypeIcon className="size-8 text-muted-foreground/40" />
          </div>
        )}
      </div>

      <div className="flex items-center gap-1.5 px-2 py-1.5">
        <TypeIcon className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="truncate text-xs text-foreground/80">{wallpaper.name}</span>
      </div>

      {wallpaper.type === "video" && (
        <div className="absolute right-1.5 top-1.5 rounded bg-black/60 px-1.5 py-0.5 text-[10px] text-white">
          {t("preview.video")}
        </div>
      )}
      {wallpaper.type === "gif" && (
        <div className="absolute right-1.5 top-1.5 rounded bg-black/60 px-1.5 py-0.5 text-[10px] text-white">
          {t("preview.gif")}
        </div>
      )}
    </div>
  );

  return (
    <ContextMenu>
      <ContextMenuTrigger>{card}</ContextMenuTrigger>
      <ContextMenuContent>
        {/* 设置为：选择显示器 */}
        <ContextMenuSub>
          <ContextMenuSubTrigger
            disabled={activeConfigs.length === 0}
            className={activeConfigs.length === 0 ? "opacity-50" : ""}
          >
            <Monitor className="mr-2 size-4" />
            {t("main.setAs")}
          </ContextMenuSubTrigger>
          <ContextMenuSubContent>
            {activeConfigs.map((config) => {
              const isCurrent = config.wallpaper_id === wallpaper.id;
              return (
                <ContextMenuItem
                  key={config.monitor_id}
                  disabled={isCurrent}
                  onClick={() => !isCurrent && handleSetAsWallpaper(config.monitor_id)}
                  className={cn(isCurrent && "opacity-50")}
                >
                  <Monitor className="mr-2 size-4 shrink-0" />
                  <span className="max-w-40 truncate">
                    {t("main.wallpaperOf", { name: config.monitor_id })}
                  </span>
                  {isCurrent && (
                    <span className="ml-auto pl-2 text-xs text-muted-foreground">
                      {t("main.currentWallpaper")}
                    </span>
                  )}
                </ContextMenuItem>
              );
            })}
          </ContextMenuSubContent>
        </ContextMenuSub>

        {/* 全部壁纸视图：添加到收藏夹 */}
        {!isCollectionView && (
          <ContextMenuSub>
            <ContextMenuSubTrigger
              disabled={collections.length === 0}
              className={collections.length === 0 ? "opacity-50" : ""}
            >
              <FolderPlus className="mr-2 size-4" />
              {t("main.addTo")}
            </ContextMenuSubTrigger>
            <ContextMenuSubContent>
              {collections.map((col) => (
                <Tooltip key={col.id}>
                  <TooltipTrigger asChild>
                    <ContextMenuItem onClick={() => onAddToCollection(wallpaper.id, col.id)}>
                      <Star className="mr-2 size-4 shrink-0" />
                      <span className="max-w-32 truncate">{col.name}</span>
                    </ContextMenuItem>
                  </TooltipTrigger>
                  <TooltipContent side="top">
                    {col.name}
                  </TooltipContent>
                </Tooltip>
              ))}
            </ContextMenuSubContent>
          </ContextMenuSub>
        )}

        {/* 删除/移除 */}
        <ContextMenuItem
          onClick={() => onDelete(wallpaper.id)}
          className={isCollectionView ? "" : "text-destructive focus:text-destructive"}
        >
          {isCollectionView ? (
            <>
              <Unlink className="mr-2 size-4" />
              {t("main.removeFromCollection")}
            </>
          ) : (
            <>
              <Trash2 className="mr-2 size-4" />
              {t("main.delete")}
            </>
          )}
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
};

/** 普通壁纸卡片（无拖拽） */
const WallpaperCard: React.FC<WallpaperCardProps> = (props) => {
  return <WallpaperCardContent {...props} />;
};

/** 可排序壁纸卡片（dnd-kit sortable） */
const SortableWallpaperCard: React.FC<WallpaperCardProps> = (props) => {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: props.wallpaper.id });

  const style: React.CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
    zIndex: isDragging ? 50 : undefined,
    position: "relative" as const,
  };

  return (
    <div ref={setNodeRef} style={style} {...attributes}>
      <WallpaperCardContent
        {...props}
        isDragging={isDragging}
        dragHandleProps={listeners}
      />
    </div>
  );
};

export default MainContent;