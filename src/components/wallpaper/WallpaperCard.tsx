import {
  Check,
  GripVertical,
} from "lucide-react";
import { memo, useCallback, useMemo } from "react";
import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { cn } from "@/lib/utils";
import ThumbnailCard from "@/components/wallpaper/ThumbnailCard";
import { useWallpaperCardContextMenu } from "@/components/wallpaper/WallpaperCardContextMenu";
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

  return (
    <ThumbnailCard
      wallpaper={wallpaper}
      style={style}
      className={cn(
        manageMode && selected
          ? "border-primary/70 ring-1 ring-primary/30 bg-primary/3"
          : "",
        isDragging && "opacity-50 fluent-shadow-lg ring-1 ring-primary/40",
      )}
      onClick={(e) => {
        if (!isDragging) onClick(wallpaper, index, e);
      }}
      onContextMenu={handleContextMenu}
      overlayTopLeft={overlayTopLeft}
      overlayBottomRight={overlayBottomRight}
    />
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
      prev.isCollectionView === next.isCollectionView
    );
  },
);
SortableWallpaperCard.displayName = "SortableWallpaperCard";
