import { Check } from "lucide-react";
import { memo, useCallback, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import ThumbnailCard from "@/components/wallpaper/ThumbnailCard";
import type { Wallpaper } from "@/api/config";
import { sortWallpapers } from "@/lib/sort";
import type { SortField, SortOrder } from "@/lib/sort";
import VirtualGrid from "./VirtualGrid";
import { FilterBar } from "./FilterBar";

// ============ 类型定义 ============

export type GridMode = "browse" | "select";

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

// ============ SelectableCard 组件（select 模式下的卡片） ============

interface SelectableCardProps {
  wallpaper: Wallpaper;
  selected: boolean;
  disabled: boolean;
  onClick: () => void;
}

const SelectableCard: React.FC<SelectableCardProps> = memo(({
  wallpaper,
  selected,
  disabled,
  onClick,
}) => {
  const { t } = useTranslation();

  // 左上角叠加层
  const overlayTopLeft = useMemo(() => {
    if (selected && !disabled) {
      return (
        <div className="absolute left-1.5 top-1.5 z-10 flex size-5 items-center justify-center rounded-full bg-primary text-primary-foreground shadow-sm">
          <Check className="size-3" />
        </div>
      );
    }
    if (!selected && !disabled) {
      return (
        <div className="absolute left-1.5 top-1.5 z-10 flex size-5 items-center justify-center rounded-full border-2 border-white/60 bg-black/20 opacity-0 transition-opacity group-hover:opacity-100" />
      );
    }
    if (disabled) {
      return (
        <div className="absolute left-1.5 top-1.5 z-10 rounded bg-black/60 px-1.5 py-0.5 text-[10px] text-white">
          {t("grid.alreadyAdded")}
        </div>
      );
    }
    return null;
  }, [selected, disabled, t]);

  return (
    <ThumbnailCard
      wallpaper={wallpaper}
      disabled={disabled}
      className={cn(
        disabled
          ? "opacity-50"
          : selected
            ? "border-primary/70 ring-1 ring-primary/30 bg-primary/3"
            : "",
      )}
      onClick={() => onClick()}
      overlayTopLeft={overlayTopLeft}
    />
  );
});
SelectableCard.displayName = "SelectableCard";

// ============ BrowseCard 组件（browse 模式下的默认卡片） ============

interface BrowseCardProps {
  wallpaper: Wallpaper;
  onClick: () => void;
}

const BrowseCard: React.FC<BrowseCardProps> = memo(({ wallpaper, onClick }) => {
  return (
    <ThumbnailCard
      wallpaper={wallpaper}
      onClick={onClick}
    />
  );
});
BrowseCard.displayName = "BrowseCard";

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

  // 用于通知 VirtualGrid 选中状态变化的版本号
  const renderVersionRef = useRef(0);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const renderVersion = useMemo(() => ++renderVersionRef.current, [selectedIds, disabledIds]);

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
              <p className="text-sm text-foreground/40">
                {keyword ? t("grid.noResults") : t("grid.empty")}
              </p>
            </div>
          )
        ) : (
          <VirtualGrid
            items={filteredWallpapers}
            getKey={(wp) => wp.id}
            className="h-full"
            renderVersion={renderVersion}
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

export default WallpaperGrid;