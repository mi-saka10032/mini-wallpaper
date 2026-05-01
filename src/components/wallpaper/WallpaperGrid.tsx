import { convertFileSrc } from "@tauri-apps/api/core";
import { Check, Film, Image } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import LazyImage from "@/components/ui/LazyImage";
import type { Wallpaper } from "@/api/config";
import VirtualGrid from "./VirtualGrid";
import { FilterBar } from "./FilterBar";

// ============ 类型定义 ============

export type GridMode = "browse" | "select";

export type SortField = "name" | "created_at" | "file_size" | "type";
export type SortOrder = "asc" | "desc";

export interface WallpaperGridProps {
  /** 壁纸数据源 */
  wallpapers: Wallpaper[];
  /** 模式：browse（浏览）或 select（多选） */
  mode: GridMode;
  /** select 模式下已选中的 ID 集合 */
  selectedIds?: Set<number>;
  /** select 模式下已在收藏夹中的 ID（显示但禁用） */
  disabledIds?: Set<number>;
  /** select 模式下选中/取消选中回调 */
  onSelectionChange?: (ids: Set<number>) => void;
  /** browse 模式下点击卡片回调 */
  onCardClick?: (wallpaper: Wallpaper, index: number) => void;
  /** 自定义卡片渲染（用于 browse 模式下的右键菜单等） */
  renderCard?: (wallpaper: Wallpaper, index: number) => React.ReactNode;
  /** 是否显示 FilterBar，默认 true */
  showFilter?: boolean;
  /** 是否显示全选/取消全选操作栏（select 模式下有效），默认 false */
  showSelectActions?: boolean;
  /** 空状态自定义内容 */
  emptyContent?: React.ReactNode;
  /** 额外的 className */
  className?: string;
}

// ============ 排序逻辑 ============

export function sortWallpapers(
  wallpapers: Wallpaper[],
  field: SortField,
  order: SortOrder,
): Wallpaper[] {
  const sorted = [...wallpapers].sort((a, b) => {
    let cmp = 0;
    switch (field) {
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
    return order === "asc" ? cmp : -cmp;
  });
  return sorted;
}

// FilterBar 已拆分至 ./FilterBar.tsx
export { FilterBar } from "./FilterBar";

// ============ SelectableCard 组件（select 模式下的卡片） ============

interface SelectableCardProps {
  wallpaper: Wallpaper;
  selected: boolean;
  disabled: boolean;
  onClick: () => void;
}

const SelectableCard: React.FC<SelectableCardProps> = ({
  wallpaper,
  selected,
  disabled,
  onClick,
}) => {
  const { t } = useTranslation();
  const TypeIcon = wallpaper.type === "video" ? Film : Image;

  return (
    <div
      className={cn(
        "group relative cursor-pointer overflow-hidden rounded-lg border bg-muted/30 transition-all",
        disabled
          ? "cursor-not-allowed border-border opacity-50"
          : selected
            ? "border-primary ring-2 ring-primary"
            : "border-border hover:ring-2 hover:ring-primary/50",
      )}
      onClick={() => !disabled && onClick()}
    >
      {/* 选中指示器 */}
      {selected && !disabled && (
        <div className="absolute left-1.5 top-1.5 z-10 flex size-5 items-center justify-center rounded-full bg-primary text-primary-foreground shadow-sm">
          <Check className="size-3" />
        </div>
      )}

      {/* 未选中时的空圆圈（hover 显示） */}
      {!selected && !disabled && (
        <div className="absolute left-1.5 top-1.5 z-10 flex size-5 items-center justify-center rounded-full border-2 border-white/60 bg-black/20 opacity-0 transition-opacity group-hover:opacity-100" />
      )}

      {/* 已在收藏夹标签 */}
      {disabled && (
        <div className="absolute left-1.5 top-1.5 z-10 rounded bg-black/60 px-1.5 py-0.5 text-[10px] text-white">
          {t("grid.alreadyAdded")}
        </div>
      )}

      {/* 缩略图 */}
      <div className="aspect-video">
        {wallpaper.thumb_path ? (
          <LazyImage
            src={convertFileSrc(wallpaper.thumb_path)}
            alt={wallpaper.name}
            fallback={<TypeIcon className="size-8 text-muted-foreground/40" />}
          />
        ) : (
          <div className="flex size-full items-center justify-center bg-muted">
            <TypeIcon className="size-8 text-muted-foreground/40" />
          </div>
        )}
      </div>

      {/* 文件信息 */}
      <div className="flex items-center gap-1.5 px-2 py-1.5">
        <TypeIcon className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="truncate text-xs text-foreground/80">{wallpaper.name}</span>
      </div>

      {/* 类型角标 */}
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
};

// ============ WallpaperGrid 主组件 ============

const WallpaperGrid: React.FC<WallpaperGridProps> = ({
  wallpapers,
  mode,
  selectedIds = new Set(),
  disabledIds = new Set(),
  onSelectionChange,
  onCardClick,
  renderCard,
  showFilter = true,
  showSelectActions = false,
  emptyContent,
  className,
}) => {
  const { t } = useTranslation();

  // FilterBar 状态
  const [keyword, setKeyword] = useState("");
  const [sortField, setSortField] = useState<SortField>("created_at");
  const [sortOrder, setSortOrder] = useState<SortOrder>("desc");

  // 过滤 + 排序后的壁纸列表
  const filteredWallpapers = useMemo(() => {
    let result = wallpapers;

    // 关键词过滤
    if (keyword.trim()) {
      const kw = keyword.trim().toLowerCase();
      result = result.filter((w) => w.name.toLowerCase().includes(kw));
    }

    // 排序
    result = sortWallpapers(result, sortField, sortOrder);

    return result;
  }, [wallpapers, keyword, sortField, sortOrder]);

  // select 模式下的 toggle 选中
  const handleToggleSelect = useCallback(
    (id: number) => {
      const next = new Set(selectedIds);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      onSelectionChange?.(next);
    },
    [selectedIds, onSelectionChange],
  );

  // 全选：选中所有可选的（排除 disabled 的）
  const selectableWallpapers = useMemo(
    () => filteredWallpapers.filter((w) => !disabledIds.has(w.id)),
    [filteredWallpapers, disabledIds],
  );

  const handleSelectAll = useCallback(() => {
    const allIds = new Set(selectableWallpapers.map((w) => w.id));
    onSelectionChange?.(allIds);
  }, [selectableWallpapers, onSelectionChange]);

  const handleClearSelection = useCallback(() => {
    onSelectionChange?.(new Set());
  }, [onSelectionChange]);

  const isEmpty = filteredWallpapers.length === 0;

  return (
    <div className={cn("flex flex-col overflow-hidden", className)}>
      {/* FilterBar */}
      {showFilter && (
        <FilterBar
          keyword={keyword}
          onKeywordChange={setKeyword}
          sortField={sortField}
          onSortFieldChange={setSortField}
          sortOrder={sortOrder}
          onSortOrderChange={setSortOrder}
          totalCount={wallpapers.length}
          filteredCount={filteredWallpapers.length}
          showSelectActions={showSelectActions && mode === "select"}
          selectedCount={selectedIds.size}
          selectableCount={selectableWallpapers.length}
          onSelectAll={handleSelectAll}
          onClearSelection={handleClearSelection}
        />
      )}

      {/* 网格内容 */}
      <div className="flex-1 overflow-hidden p-4">
        {isEmpty ? (
          emptyContent || (
            <div className="flex h-full min-h-40 items-center justify-center">
              <p className="text-sm text-muted-foreground">
                {keyword ? t("grid.noResults") : t("grid.empty")}
              </p>
            </div>
          )
        ) : (
          <VirtualGrid
            items={filteredWallpapers}
            getKey={(wp) => wp.id}
            className="h-full"
            renderItem={(wp, index) => {
              if (mode === "select") {
                return (
                  <SelectableCard
                    wallpaper={wp}
                    selected={selectedIds.has(wp.id)}
                    disabled={disabledIds.has(wp.id)}
                    onClick={() => handleToggleSelect(wp.id)}
                  />
                );
              }

              // browse 模式：使用自定义渲染或默认卡片
              if (renderCard) {
                return renderCard(wp, index);
              }

              return (
                <BrowseCard
                  wallpaper={wp}
                  onClick={() => onCardClick?.(wp, index)}
                />
              );
            }}
          />
        )}
      </div>
    </div>
  );
};

// ============ BrowseCard 组件（browse 模式下的默认卡片） ============

interface BrowseCardProps {
  wallpaper: Wallpaper;
  onClick: () => void;
}

const BrowseCard: React.FC<BrowseCardProps> = ({ wallpaper, onClick }) => {
  const { t } = useTranslation();
  const TypeIcon = wallpaper.type === "video" ? Film : Image;

  return (
    <div
      className="group relative cursor-pointer overflow-hidden rounded-lg border border-border bg-muted/30 transition-all hover:ring-2 hover:ring-primary/50"
      onClick={onClick}
    >
      <div className="aspect-video">
        {wallpaper.thumb_path ? (
          <LazyImage
            src={convertFileSrc(wallpaper.thumb_path)}
            alt={wallpaper.name}
            fallback={<TypeIcon className="size-8 text-muted-foreground/40" />}
          />
        ) : (
          <div className="flex size-full items-center justify-center bg-muted">
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
};

export default WallpaperGrid;