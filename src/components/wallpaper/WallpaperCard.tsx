import {
  Check,
  FolderPlus,
  GripVertical,
  Monitor,
  Star,
  Trash2,
  Unlink,
} from "lucide-react";
import { memo, useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import type { Collection } from "@/stores/collectionStore";
import type { MonitorConfig } from "@/api/config";
import { useMonitorConfigStore } from "@/stores/monitorConfigStore";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import ThumbnailCard from "@/components/wallpaper/ThumbnailCard";
import type { Wallpaper } from "@/api/config";
import { getCollectionWallpapers } from "@/api/collection";

// ============ 类型定义 ============

export interface WallpaperCardProps {
  wallpaper: Wallpaper;
  index: number;
  activeId: number;
  manageMode: boolean;
  selected: boolean;
  isCollectionView: boolean;
  /** 从父组件传入的活跃显示器配置列表 */
  activeConfigs: MonitorConfig[];
  /** 从父组件传入的收藏夹列表 */
  collections: Collection[];
  /** 从父组件传入的显示模式（independent/mirror/extend） */
  displayMode: string;
  /** 从父组件传入的 monitorConfigStore.upsert */
  upsert: ReturnType<typeof useMonitorConfigStore.getState>["upsert"];
  /** 从父组件传入的 monitorConfigStore.upsertAll */
  upsertAll: ReturnType<typeof useMonitorConfigStore.getState>["upsertAll"];
  isDragging?: boolean;
  dragHandleProps?: React.HTMLAttributes<HTMLDivElement>;
  onClick: (wp: Wallpaper, index: number, e: React.MouseEvent) => void;
  onDelete: (id: number) => void;
  onAddToCollection: (wallpaperId: number, collectionId: number) => void;
}

// ============ useSetAsWallpaper Hook ============

/**
 * 处理"设置为 X 显示器的壁纸"的逻辑
 * 根据 activeId（0=本地壁纸栏, >0=收藏夹栏）和目标显示器的 config 状态，分 6 种场景处理
 */
function useSetAsWallpaper(
  wallpaperId: number,
  activeId: number,
  configs: MonitorConfig[],
  displayMode: string,
  upsert: ReturnType<typeof useMonitorConfigStore.getState>["upsert"],
  upsertAll: ReturnType<typeof useMonitorConfigStore.getState>["upsertAll"],
) {
  const isSyncMode = displayMode === "mirror" || displayMode === "extend";

  const handleSetAsWallpaper = useCallback(
    async (monitorId: string) => {
      const targetConfig = configs.find((c) => c.monitor_id === monitorId);
      const collectionId = activeId > 0 ? activeId : null;

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
    [wallpaperId, activeId, configs, isSyncMode, upsert, upsertAll],
  );

  return handleSetAsWallpaper;
}

// ============ WallpaperCardContent 组件 ============

/** 单个壁纸卡片（内容渲染） */
const WallpaperCardContent: React.FC<WallpaperCardProps & { style?: React.CSSProperties }> = ({
  wallpaper,
  index,
  activeId,
  manageMode,
  selected,
  isCollectionView,
  activeConfigs,
  collections,
  displayMode,
  upsert,
  upsertAll,
  isDragging = false,
  dragHandleProps,
  onClick,
  onDelete,
  onAddToCollection,
  style,
}) => {
  const { t } = useTranslation();

  const handleSetAsWallpaper = useSetAsWallpaper(wallpaper.id, activeId, activeConfigs, displayMode, upsert, upsertAll);

  // 左上角叠加层：选中指示器
  const overlayTopLeft = useMemo(() => {
    if (manageMode && selected) {
      return (
        <div className="absolute left-1.5 top-1.5 z-10 flex size-5 items-center justify-center rounded-full bg-primary text-primary-foreground">
          <Check className="size-3" />
        </div>
      );
    }
    if (manageMode && !selected) {
      return (
        <div className="absolute left-1.5 top-1.5 z-10 flex size-5 items-center justify-center rounded-full border-2 border-white/60 bg-black/20 opacity-0 transition-opacity group-hover:opacity-100" />
      );
    }
    return null;
  }, [manageMode, selected]);

  // 右下角叠加层：拖拽手柄
  const overlayBottomRight = useMemo(() => {
    if (!dragHandleProps) return null;
    return (
      <div
        {...dragHandleProps}
        onClick={(e) => e.stopPropagation()}
        className="absolute right-1.5 bottom-8 z-20 flex size-6 cursor-grab items-center justify-center rounded bg-black/40 text-white opacity-0 transition-opacity active:cursor-grabbing group-hover:opacity-100"
      >
        <GripVertical className="size-3.5" />
      </div>
    );
  }, [dragHandleProps]);

  const card = (
    <ThumbnailCard
      wallpaper={wallpaper}
      style={style}
      className={cn(
        manageMode && selected
          ? "border-primary ring-2 ring-primary"
          : "border-border hover:ring-2 hover:ring-primary/50",
        isDragging && "opacity-50 shadow-lg ring-2 ring-primary",
      )}
      onClick={(e) => {
        if (!isDragging) onClick(wallpaper, index, e);
      }}
      overlayTopLeft={overlayTopLeft}
      overlayBottomRight={overlayBottomRight}
    />
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

// ============ 导出组件（带 React.memo） ============

/** 普通壁纸卡片（无拖拽），使用 React.memo 避免不必要的重渲染 */
export const WallpaperCard = memo(
  WallpaperCardContent,
  (prev, next) => {
    // 仅在影响渲染的 props 变化时才重渲染
    return (
      prev.wallpaper === next.wallpaper &&
      prev.index === next.index &&
      prev.activeId === next.activeId &&
      prev.manageMode === next.manageMode &&
      prev.selected === next.selected &&
      prev.isCollectionView === next.isCollectionView &&
      prev.activeConfigs === next.activeConfigs &&
      prev.collections === next.collections &&
      prev.displayMode === next.displayMode &&
      prev.upsert === next.upsert &&
      prev.upsertAll === next.upsertAll &&
      prev.onClick === next.onClick &&
      prev.onDelete === next.onDelete &&
      prev.onAddToCollection === next.onAddToCollection
    );
  },
);
WallpaperCard.displayName = "WallpaperCard";

/** 可排序壁纸卡片（dnd-kit sortable） */
const SortableWallpaperCardInner: React.FC<WallpaperCardProps> = (props) => {
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

export const SortableWallpaperCard = memo(
  SortableWallpaperCardInner,
  (prev, next) => {
    // 排序模式下仅关注数据变化，跳过回调引用比较
    return (
      prev.wallpaper === next.wallpaper &&
      prev.index === next.index &&
      prev.activeId === next.activeId &&
      prev.manageMode === next.manageMode &&
      prev.selected === next.selected &&
      prev.isCollectionView === next.isCollectionView &&
      prev.activeConfigs === next.activeConfigs &&
      prev.collections === next.collections &&
      prev.displayMode === next.displayMode
    );
  },
);
SortableWallpaperCard.displayName = "SortableWallpaperCard";