import { Check, GripVertical, Heart, Monitor } from "lucide-react";
import { memo, startTransition, useCallback, useMemo, useOptimistic } from "react";
import { useTranslation } from "react-i18next";
import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { cn } from "@/lib/utils";
import ThumbnailCard from "@/components/wallpaper/ThumbnailCard";
import { useWallpaperCardContextMenu } from "@/components/wallpaper/WallpaperCardContextMenu";
import { useFavoritesStore } from "@/stores/favoritesStore";
import { useMonitorsUsingWallpaper } from "@/hooks/useMonitorsUsingWallpaper";
import type { Wallpaper } from "@/api/config";

// ============ 类型定义 ============

export interface WallpaperCardProps {
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

// ============ WallpaperCardContent 组件 ============

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
  const { openContextMenu } = useWallpaperCardContextMenu();
  const { t } = useTranslation();

  // 订阅该壁纸的收藏状态（仅当自身收藏态变化时重渲染）
  const isFavorite = useFavoritesStore((s) => s.favoriteIds.has(wallpaper.id));
  const toggleFavorite = useFavoritesStore((s) => s.toggle);

  // 红心点击走乐观更新：后端 toggle 完成后 store 会经 applyChange / 事件回流
  // 更新，useOptimistic 自动回落到真实值；失败则回落到旧值，无需手动回滚。
  const [optimisticFavorite, setOptimisticFavorite] = useOptimistic(
    isFavorite,
    (_current: boolean, next: boolean) => next,
  );

  // 反查该壁纸正被哪些 active 显示器使用（多显示器为默认场景，单显示器同样返回该显示器）
  const monitorsInUse = useMonitorsUsingWallpaper(wallpaper.id);

  const handleToggleFavorite = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      startTransition(async () => {
        setOptimisticFavorite(!optimisticFavorite);
        try {
          await toggleFavorite(wallpaper.id);
        } catch (err) {
          console.error("[toggleFavorite]", err);
        }
      });
    },
    [optimisticFavorite, toggleFavorite, wallpaper.id],
  );

  // 右键菜单处理：仅在卡片范围内触发
  const handleContextMenu = useCallback(
    (e: React.MouseEvent) => {
      openContextMenu(wallpaper, e, {
        activeId,
        isCollectionView,
        onDelete,
        onAddToCollection,
      });
    },
    [wallpaper, activeId, isCollectionView, onDelete, onAddToCollection, openContextMenu],
  );

  // 左上角叠加层：选中指示器
  const overlayTopLeft = useMemo(() => {
    if (manageMode && selected) {
      return (
        <div className="absolute left-1.5 top-1.5 z-10 flex size-5 items-center justify-center rounded-full bg-primary text-primary-foreground shadow-sm">
          <Check className="size-3" />
        </div>
      );
    }
    if (manageMode && !selected) {
      return (
        <div className="absolute left-1.5 top-1.5 z-10 flex size-5 items-center justify-center rounded-full border-2 border-white/50 bg-black/15 opacity-0 transition-opacity group-hover:opacity-100" />
      );
    }
    // 非管理模式：红心一键收藏按钮（已收藏常亮，未收藏 hover 显示）
    return (
      <button
        type="button"
        onClick={handleToggleFavorite}
        title={optimisticFavorite ? t("main.unfavorite") : t("main.favorite")}
        className={cn(
          "absolute left-1.5 top-1.5 z-20 flex size-6 items-center justify-center rounded-full bg-black/40 backdrop-blur-sm transition-[background-color,transform] hover:bg-black/60 hover:scale-110 active:scale-95",
          optimisticFavorite
            ? "opacity-100 text-red-500"
            : "text-white/90 opacity-0 group-hover:opacity-100",
        )}
      >
        <Heart className={cn("size-3.5", optimisticFavorite && "fill-current")} />
      </button>
    );
  }, [manageMode, selected, optimisticFavorite, handleToggleFavorite, t]);

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

  // 缩略图左下角叠加层：「使用中」角标（常驻，无需右键即可识别当前壁纸）
  const overlayBottomLeft = useMemo(() => {
    if (monitorsInUse.length === 0) return null;
    // 管理模式下让位于勾选态，避免与批量操作的视觉信号竞争
    if (manageMode) return null;

    const isMultiple = monitorsInUse.length > 1;
    const label = isMultiple
      ? t("main.inUse") + " · " + monitorsInUse.length
      : t("main.inUse") + " · " + monitorsInUse[0];
    const tooltip = isMultiple
      ? t("main.inUseOnMultiple", {
          count: monitorsInUse.length,
          names: monitorsInUse.join("、"),
        })
      : t("main.inUseOn", { name: monitorsInUse[0] });

    return (
      <div
        title={tooltip}
        className="absolute bottom-1.5 left-1.5 z-10 flex max-w-[calc(100%-0.75rem)] items-center gap-1 rounded-sm bg-primary/90 px-1.5 py-0.5 text-[10px] font-medium text-primary-foreground shadow-sm backdrop-blur-sm"
      >
        <Monitor className="size-3 shrink-0" />
        <span className="truncate">{label}</span>
      </div>
    );
  }, [monitorsInUse, manageMode, t]);

  return (
    <ThumbnailCard
      wallpaper={wallpaper}
      style={style}
      className={cn(
        manageMode && selected ? "border-primary/70 ring-1 ring-primary/30 bg-primary/3" : "",
        !manageMode && monitorsInUse.length > 0 && "border-primary/60 ring-1 ring-primary/25",
        isDragging && "opacity-50 fluent-shadow-lg ring-1 ring-primary/40",
      )}
      onClick={(e) => {
        if (!isDragging) onClick(wallpaper, index, e);
      }}
      onContextMenu={handleContextMenu}
      overlayTopLeft={overlayTopLeft}
      overlayBottomRight={overlayBottomRight}
      overlayBottomLeft={overlayBottomLeft}
    />
  );
};

// ============ 导出组件（带 React.memo） ============

/** 普通壁纸卡片（无拖拽），使用 React.memo 避免不必要的重渲染 */
export const WallpaperCard = memo(WallpaperCardContent, (prev, next) => {
  // 仅在影响渲染的 props 变化时才重渲染
  return (
    prev.wallpaper === next.wallpaper &&
    prev.index === next.index &&
    prev.activeId === next.activeId &&
    prev.manageMode === next.manageMode &&
    prev.selected === next.selected &&
    prev.isCollectionView === next.isCollectionView &&
    prev.onClick === next.onClick &&
    prev.onDelete === next.onDelete &&
    prev.onAddToCollection === next.onAddToCollection
  );
});
WallpaperCard.displayName = "WallpaperCard";

/** 可排序壁纸卡片（dnd-kit sortable） */
const SortableWallpaperCardInner: React.FC<WallpaperCardProps> = (props) => {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: props.wallpaper.id,
  });

  const style: React.CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
    zIndex: isDragging ? 50 : undefined,
    position: "relative" as const,
  };

  return (
    <div ref={setNodeRef} style={style} {...attributes}>
      <WallpaperCardContent {...props} isDragging={isDragging} dragHandleProps={listeners} />
    </div>
  );
};

export const SortableWallpaperCard = memo(SortableWallpaperCardInner, (prev, next) => {
  // 排序模式下仅关注数据变化，跳过回调引用比较
  return (
    prev.wallpaper === next.wallpaper &&
    prev.index === next.index &&
    prev.activeId === next.activeId &&
    prev.manageMode === next.manageMode &&
    prev.selected === next.selected &&
    prev.isCollectionView === next.isCollectionView
  );
});
SortableWallpaperCard.displayName = "SortableWallpaperCard";
