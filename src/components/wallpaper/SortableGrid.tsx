import {
  DndContext,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  SortableContext,
  rectSortingStrategy,
} from "@dnd-kit/sortable";
import type { DragEndEvent } from "@dnd-kit/core";
import { SortableWallpaperCard } from "@/components/wallpaper/WallpaperCard";
import type { Wallpaper } from "@/api/config";

interface SortableGridProps {
  wallpapers: Wallpaper[];
  wallpaperIds: number[];
  activeId: number;
  manageMode: boolean;
  selectedIds: Set<number>;
  isCollectionView: boolean;
  onDragEnd: (event: DragEndEvent) => void;
  onClick: (wp: Wallpaper, index: number, e: React.MouseEvent) => void;
  onDelete: (id: number) => void;
  onAddToCollection: (wallpaperId: number, collectionId: number) => void;
}

/**
 * 可排序网格组件（懒加载）
 * 将 @dnd-kit 相关逻辑封装，仅在排序模式下动态加载
 */
const SortableGrid: React.FC<SortableGridProps> = ({
  wallpapers,
  wallpaperIds,
  activeId,
  manageMode,
  selectedIds,
  isCollectionView,
  onDragEnd,
  onClick,
  onDelete,
  onAddToCollection,
}) => {
  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: { distance: 10 },
    }),
  );

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragEnd={onDragEnd}
    >
      <SortableContext items={wallpaperIds} strategy={rectSortingStrategy}>
        <div className="grid grid-cols-3 gap-3 xl:grid-cols-4 2xl:grid-cols-5">
          {wallpapers.map((wp, index) => (
            <SortableWallpaperCard
              key={wp.id}
              wallpaper={wp}
              index={index}
              activeId={activeId}
              manageMode={manageMode}
              selected={selectedIds.has(wp.id)}
              isCollectionView={isCollectionView}
              onClick={onClick}
              onDelete={onDelete}
              onAddToCollection={onAddToCollection}
            />
          ))}
        </div>
      </SortableContext>
    </DndContext>
  );
};

export default SortableGrid;
