import {
  DndContext,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import { SortableContext, useSortable, verticalListSortingStrategy } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { memo, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { Move, Heart, Pencil, Sparkles, Star, Trash2, FolderSync } from "lucide-react";
import { cn } from "@/lib/utils";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { Collection } from "@/stores/collectionStore";
import { useCollectionStore } from "@/stores/collectionStore";

interface SortableCollectionListProps {
  collections: Collection[];
  activeId: number;
  onActiveIdChange: (id: number) => void;
  onEditSmart: (collection: Collection) => void;
  onRename: (collection: Collection) => void;
  onDelete: (collection: Collection) => void;
}

interface SortableItemProps {
  collection: Collection;
  children: React.ReactNode;
}

const SortableItem: React.FC<SortableItemProps> = memo(({ collection, children }) => {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: collection.id,
  });

  return (
    <div
      ref={setNodeRef}
      style={{ transform: CSS.Translate.toString(transform), transition }}
      className={cn("relative", isDragging && "z-10 opacity-80")}
      {...attributes}
      {...listeners}
    >
      {children}
      {isDragging && (
        <Move className="pointer-events-none absolute right-1 top-1/2 size-3.5 -translate-y-1/2 text-primary" />
      )}
    </div>
  );
});
SortableItem.displayName = "SortableItem";

const SortableCollectionList: React.FC<SortableCollectionListProps> = memo(
  ({ collections, activeId, onActiveIdChange, onEditSmart, onRename, onDelete }) => {
    const { t } = useTranslation();
    const reorderCollections = useCollectionStore((s) => s.reorderCollections);

    const sensors = useSensors(
      useSensor(PointerSensor, {
        activationConstraint: { delay: 500, tolerance: 5 },
      }),
    );

    const builtin = collections.filter((c) => c.is_builtin === 1);
    const custom = collections.filter((c) => c.is_builtin !== 1);

    const handleDragEnd = useCallback(
      (event: DragEndEvent) => {
        const { active, over } = event;
        if (!over || active.id === over.id) return;

        const oldIndex = custom.findIndex((c) => c.id === active.id);
        const newIndex = custom.findIndex((c) => c.id === over.id);
        if (oldIndex === -1 || newIndex === -1) return;

        const updated = [...custom];
        const [moved] = updated.splice(oldIndex, 1);
        updated.splice(newIndex, 0, moved);
        void reorderCollections(updated.map((c) => c.id)).catch(() => {});
      },
      [custom, reorderCollections],
    );

    const renderItem = (collection: Collection) => {
      const isBuiltin = collection.is_builtin === 1;
      const isSmart = collection.kind === "smart";
      const isWatched = collection.kind === "watched";
      const displayName = isBuiltin ? t("sidebar.builtinFavorites") : collection.name;
      const Icon = isBuiltin ? Heart : isSmart ? Sparkles : isWatched ? FolderSync : Star;

      const trigger = (
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              onClick={() => onActiveIdChange(collection.id)}
              className={cn(
                "fluent-indicator flex w-full min-w-0 items-center gap-2 overflow-hidden rounded-md px-3 py-1.5 text-sm transition-colors duration-150",
                activeId === collection.id
                  ? "fluent-indicator-active bg-primary-hover-deep text-foreground font-medium"
                  : "text-muted-foreground hover:bg-primary-hover hover:text-foreground",
              )}
            >
              <Icon
                className={cn(
                  "size-4 shrink-0",
                  isBuiltin && "fill-[#ef4444] text-[#ef4444]",
                  !isBuiltin && isSmart && "text-primary",
                )}
              />
              <span className="block max-w-[120px] truncate">{displayName}</span>
            </button>
          </TooltipTrigger>
          <TooltipContent side="right">{displayName}</TooltipContent>
        </Tooltip>
      );

      if (isBuiltin) {
        return <div className="block w-full min-w-0">{trigger}</div>;
      }

      return (
        <ContextMenu>
          <ContextMenuTrigger className="block w-full min-w-0">{trigger}</ContextMenuTrigger>
          <ContextMenuContent className="w-32">
            {isSmart && (
              <ContextMenuItem onClick={() => onEditSmart(collection)}>
                <Sparkles className="mr-2 size-3.5" />
                {t("sidebar.editRule")}
              </ContextMenuItem>
            )}
            <ContextMenuItem onClick={() => onRename(collection)}>
              <Pencil className="mr-2 size-3.5" />
              {t("sidebar.rename")}
            </ContextMenuItem>
            {/* 监听夹只能由「移除监听」级联删除，不暴露手动删除入口 */}
            {!isWatched && (
              <ContextMenuItem
                onClick={() => onDelete(collection)}
                className="text-destructive focus:text-destructive"
              >
                <Trash2 className="mr-2 size-3.5" />
                {t("sidebar.delete")}
              </ContextMenuItem>
            )}
          </ContextMenuContent>
        </ContextMenu>
      );
    };

    return (
      <div className="space-y-0.5">
        {builtin.map((c) => (
          <div key={c.id}>{renderItem(c)}</div>
        ))}
        <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
          <SortableContext items={custom.map((c) => c.id)} strategy={verticalListSortingStrategy}>
            {custom.map((c) => (
              <SortableItem key={c.id} collection={c}>
                {renderItem(c)}
              </SortableItem>
            ))}
          </SortableContext>
        </DndContext>
        {collections.length === 0 && (
          <p className="px-3 py-2 text-xs text-foreground/40">{t("sidebar.noCollections")}</p>
        )}
      </div>
    );
  },
);
SortableCollectionList.displayName = "SortableCollectionList";

export default SortableCollectionList;
