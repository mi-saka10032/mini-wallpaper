import { convertFileSrc } from "@tauri-apps/api/core";
import { Check, Film, Image, Search, SortAsc, X } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import type { Wallpaper } from "@/api/config";

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
  /** 空状态自定义内容 */
  emptyContent?: React.ReactNode;
  /** 额外的 className */
  className?: string;
}

// ============ 排序逻辑 ============

function sortWallpapers(
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

// ============ FilterBar 组件 ============

interface FilterBarProps {
  keyword: string;
  onKeywordChange: (value: string) => void;
  sortField: SortField;
  onSortFieldChange: (field: SortField) => void;
  sortOrder: SortOrder;
  onSortOrderChange: (order: SortOrder) => void;
  totalCount: number;
  filteredCount: number;
}

const FilterBar: React.FC<FilterBarProps> = ({
  keyword,
  onKeywordChange,
  sortField,
  onSortFieldChange,
  sortOrder,
  onSortOrderChange,
  totalCount,
  filteredCount,
}) => {
  const { t } = useTranslation();

  return (
    <div className="flex items-center gap-2 border-b border-border px-4 py-2">
      {/* 搜索框 */}
      <div className="relative flex-1 max-w-xs">
        <Search className="absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
        <Input
          value={keyword}
          onChange={(e) => onKeywordChange(e.target.value)}
          placeholder={t("grid.searchPlaceholder")}
          className="h-8 pl-8 pr-8 text-sm"
        />
        {keyword && (
          <button
            type="button"
            onClick={() => onKeywordChange("")}
            className="absolute right-2 top-1/2 -translate-y-1/2 rounded-sm p-0.5 text-muted-foreground hover:text-foreground"
          >
            <X className="size-3" />
          </button>
        )}
      </div>

      {/* 排序选择 */}
      <Select value={sortField} onValueChange={(v) => onSortFieldChange(v as SortField)}>
        <SelectTrigger size="sm" className="h-8 w-auto gap-1.5 text-xs">
          <SortAsc className="size-3.5" />
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="created_at">{t("grid.sortByDate")}</SelectItem>
          <SelectItem value="name">{t("grid.sortByName")}</SelectItem>
          <SelectItem value="file_size">{t("grid.sortBySize")}</SelectItem>
          <SelectItem value="type">{t("grid.sortByType")}</SelectItem>
        </SelectContent>
      </Select>

      {/* 排序方向 */}
      <button
        type="button"
        onClick={() => onSortOrderChange(sortOrder === "asc" ? "desc" : "asc")}
        className={cn(
          "flex size-8 items-center justify-center rounded-md border border-input text-muted-foreground transition-colors hover:bg-accent hover:text-foreground",
          sortOrder === "desc" && "rotate-180",
        )}
        title={sortOrder === "asc" ? t("grid.ascending") : t("grid.descending")}
      >
        <SortAsc className="size-3.5" />
      </button>

      {/* 筛选结果计数 */}
      {keyword && filteredCount !== totalCount && (
        <span className="text-xs text-muted-foreground">
          {t("grid.filterResult", { filtered: filteredCount, total: totalCount })}
        </span>
      )}
    </div>
  );
};

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
        />
      )}

      {/* 网格内容 */}
      <div className="flex-1 overflow-y-auto p-4">
        {isEmpty ? (
          emptyContent || (
            <div className="flex h-full min-h-40 items-center justify-center">
              <p className="text-sm text-muted-foreground">
                {keyword ? t("grid.noResults") : t("grid.empty")}
              </p>
            </div>
          )
        ) : (
          <div className="grid grid-cols-3 gap-3 xl:grid-cols-4 2xl:grid-cols-5">
            {filteredWallpapers.map((wp, index) => {
              if (mode === "select") {
                return (
                  <SelectableCard
                    key={wp.id}
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
                  key={wp.id}
                  wallpaper={wp}
                  onClick={() => onCardClick?.(wp, index)}
                />
              );
            })}
          </div>
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
};

export default WallpaperGrid;
