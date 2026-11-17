import { Search, SortAsc, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { TagWithCount } from "@/api/config";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import type { SortField, SortOrder } from "@/lib/sort";
import TagSelect from "./TagSelect";

// ============ 类型定义 ============

export interface FilterBarProps {
  keyword: string;
  onKeywordChange: (value: string) => void;
  sortField: SortField;
  onSortFieldChange: (field: SortField) => void;
  sortOrder: SortOrder;
  onSortOrderChange: (order: SortOrder) => void;
  totalCount: number;
  filteredCount: number;
  /** 标签筛选选项；传入才渲染标签筛选控件，空数组时控件禁用（库内无标签） */
  tagOptions?: TagWithCount[];
  /** 已选标签 id 集合（受控） */
  selectedTagIds?: Set<number>;
  /** 标签选择变化 */
  onSelectedTagIdsChange?: (ids: Set<number>) => void;
  /** select 模式下的全选/取消全选操作 */
  showSelectActions?: boolean;
  selectedCount?: number;
  selectableCount?: number;
  onSelectAll?: () => void;
  onClearSelection?: () => void;
}

// ============ FilterBar 组件 ============

export const FilterBar: React.FC<FilterBarProps> = ({
  keyword,
  onKeywordChange,
  sortField,
  onSortFieldChange,
  sortOrder,
  onSortOrderChange,
  totalCount,
  filteredCount,
  tagOptions,
  selectedTagIds,
  onSelectedTagIdsChange,
  showSelectActions = false,
  selectedCount = 0,
  selectableCount = 0,
  onSelectAll,
  onClearSelection,
}) => {
  const { t } = useTranslation();

  // 任一筛选生效（关键词或标签）且收窄了结果时显示计数
  const hasActiveFilter = Boolean(keyword) || (selectedTagIds?.size ?? 0) > 0;

  return (
    <div className="flex items-center gap-2 border-b border-border/50 px-4 py-2">
      {/* 全选/取消全选操作（select 模式） */}
      {showSelectActions && (
        <>
          <button
            type="button"
            onClick={onSelectAll}
            disabled={selectableCount === 0}
            className="text-xs text-primary/80 hover:text-primary hover:underline disabled:text-foreground/40 disabled:no-underline"
          >
            {t("grid.selectAll")}
          </button>
          {selectedCount > 0 && (
            <button
              type="button"
              onClick={onClearSelection}
              className="text-xs text-primary/80 hover:text-primary hover:underline"
            >
              {t("grid.clearSelection")}
            </button>
          )}
          <div className="mx-1 h-4 w-px bg-border/30" />
        </>
      )}

      {/* 搜索框 */}
      <div className="relative flex-1 max-w-xs">
        <Search className="absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-foreground/40" />
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
            className="absolute right-2 top-1/2 -translate-y-1/2 rounded-sm p-0.5 text-foreground/40 hover:text-foreground"
          >
            <X className="size-3" />
          </button>
        )}
      </div>

      {/* 标签筛选（仅选择器场景传入 tagOptions 时渲染；Select 形态宽度恒定，不撑高工具栏） */}
      {tagOptions && selectedTagIds && onSelectedTagIdsChange && (
        <TagSelect
          options={tagOptions}
          selectedIds={selectedTagIds}
          onChange={onSelectedTagIdsChange}
          className="w-32"
        />
      )}

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
          "flex size-8 items-center justify-center rounded-md border border-border/50 text-muted-foreground transition-colors hover:bg-primary-hover hover:text-foreground",
          sortOrder === "desc" && "rotate-180",
        )}
        title={sortOrder === "asc" ? t("grid.ascending") : t("grid.descending")}
      >
        <SortAsc className="size-3.5" />
      </button>

      {/* 筛选结果计数 */}
      {hasActiveFilter && filteredCount !== totalCount && (
        <span className="text-xs text-foreground/40">
          {t("grid.filterResult", { filtered: filteredCount, total: totalCount })}
        </span>
      )}
    </div>
  );
};

export default FilterBar;
