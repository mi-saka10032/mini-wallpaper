import { Check } from "lucide-react";
import { memo, useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { getWallpapersByTags } from "@/api/tag";
import type { TagWithCount, Wallpaper } from "@/api/config";
import { cn } from "@/lib/utils";
import ThumbnailCard from "@/components/wallpaper/ThumbnailCard";
import type { SortField, SortOrder } from "@/lib/sort";
import { filterWallpapers } from "@/lib/wallpaperFilter";
import VirtualGrid from "./VirtualGrid";
import { FilterBar } from "./FilterBar";

// ============ 类型定义 ============

export interface WallpaperGridProps {
  /** 壁纸数据源 */
  wallpapers: Wallpaper[];
  /** 已选中的 ID 集合 */
  selectedIds?: Set<number>;
  /** 已在收藏夹中的 ID（显示但禁用） */
  disabledIds?: Set<number>;
  /** 选中/取消选中回调 */
  onSelectionChange?: (ids: Set<number>) => void;
  /** 是否显示 FilterBar，默认 true */
  showFilter?: boolean;
  /** 是否显示全选/取消全选操作栏，默认 false */
  showSelectActions?: boolean;
  /** 标签筛选项；传入才启用标签筛选（选择器场景），空数组时控件禁用 */
  tagOptions?: TagWithCount[];
  /** 空状态自定义内容 */
  emptyContent?: React.ReactNode;
  /** 额外的 className */
  className?: string;
}

// ============ 卡片组件 ============

interface SelectableCardProps {
  wallpaper: Wallpaper;
  selected: boolean;
  disabled: boolean;
  onClick: () => void;
}

const SelectableCard: React.FC<SelectableCardProps> = memo(
  ({ wallpaper, selected, disabled, onClick }) => {
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
  },
);
SelectableCard.displayName = "SelectableCard";

// ============ 主组件 ============

/**
 * 收藏夹「添加壁纸」选择器的网格。
 *
 * 只保留多选语义（选中 / 禁用已在夹内的项 / 全选）；历史上预留的 browse
 * 分支无任何调用方，已移除。过滤与排序复用 lib/wallpaperFilter，与主内容区同源。
 * 传入 tagOptions 即启用标签筛选：选中集由后端求值为壁纸 id 白名单，
 * 再与关键词过滤叠加（AND），语义对齐智能收藏夹的 tag includes_any。
 */
const WallpaperGrid: React.FC<WallpaperGridProps> = ({
  wallpapers,
  selectedIds = new Set(),
  disabledIds = new Set(),
  onSelectionChange,
  showFilter = true,
  showSelectActions = false,
  tagOptions,
  emptyContent,
  className,
}) => {
  const { t } = useTranslation();

  // FilterBar 状态
  const [keyword, setKeyword] = useState("");
  const [sortField, setSortField] = useState<SortField>("created_at");
  const [sortOrder, setSortOrder] = useState<SortOrder>("asc");

  // 标签筛选：选中集 → 后端求值壁纸 id 白名单（与 keyword 过滤叠加，AND 语义）
  const [selectedTagIds, setSelectedTagIds] = useState<Set<number>>(new Set());
  // null = 未启用标签过滤（未选任何 tag 或未开启该能力），Set = 白名单（空集即无命中）
  const [allowedIds, setAllowedIds] = useState<Set<number> | null>(null);

  useEffect(() => {
    if (tagOptions === undefined) return; // 未启用标签筛选
    if (selectedTagIds.size === 0) {
      setAllowedIds(null);
      return;
    }
    let cancelled = false;
    getWallpapersByTags(Array.from(selectedTagIds))
      .then((hits) => {
        if (!cancelled) setAllowedIds(new Set(hits.map((w) => w.id)));
      })
      .catch((e) => {
        // 错误已由 invoke 层统一 toast；此处按空命中处理，避免误放行全量壁纸
        console.error("[WallpaperGrid.tagFilter]", e);
        if (!cancelled) setAllowedIds(new Set());
      });
    return () => {
      cancelled = true;
    };
  }, [selectedTagIds, tagOptions]);

  const filteredWallpapers = useMemo(() => {
    let result = filterWallpapers(wallpapers, keyword, sortField, sortOrder);
    if (allowedIds) {
      result = result.filter((w) => allowedIds.has(w.id));
    }
    return result;
  }, [wallpapers, keyword, sortField, sortOrder, allowedIds]);

  // 切换选中
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
          tagOptions={tagOptions}
          selectedTagIds={selectedTagIds}
          onSelectedTagIdsChange={setSelectedTagIds}
          totalCount={wallpapers.length}
          filteredCount={filteredWallpapers.length}
          showSelectActions={showSelectActions}
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
                {keyword || allowedIds ? t("grid.noResults") : t("grid.empty")}
              </p>
            </div>
          )
        ) : (
          <VirtualGrid
            items={filteredWallpapers}
            getKey={(wp) => wp.id}
            className="h-full"
            renderItem={(wp) => (
              <SelectableCard
                wallpaper={wp}
                selected={selectedIds.has(wp.id)}
                disabled={disabledIds.has(wp.id)}
                onClick={() => handleToggleSelect(wp.id)}
              />
            )}
          />
        )}
      </div>
    </div>
  );
};

export default WallpaperGrid;
