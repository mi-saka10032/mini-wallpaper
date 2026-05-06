import { Search, SortAsc, X } from "lucide-react";
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
import type { SortField, SortOrder } from "@/lib/sort";

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
  showSelectActions = false,
  selectedCount = 0,
  selectableCount = 0,
  onSelectAll,
  onClearSelection,
}) => {
  const { t } = useTranslation();

  return (
    <div className="flex items-center gap-2 border-b border-border/40 px-4 py-2">
      {/* 全选/取消全选操作（select 模式） */}
      {showSelectActions && (
        <>
          <button
            type="button"
            onClick={onSelectAll}
            disabled={selectableCount === 0}
            className="text-xs text-primary/80 hover:text-primary hover:underline disabled:text-foreground/30 disabled:no-underline"
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
          <div className="mx-1 h-4 w-px bg-border/50" />
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
          "flex size-8 items-center justify-center rounded-md border border-border/50 text-foreground/60 transition-colors hover:bg-primary/8 hover:text-foreground",
          sortOrder === "desc" && "rotate-180",
        )}
        title={sortOrder === "asc" ? t("grid.ascending") : t("grid.descending")}
      >
        <SortAsc className="size-3.5" />
      </button>

      {/* 筛选结果计数 */}
      {keyword && filteredCount !== totalCount && (
        <span className="text-xs text-foreground/45">
          {t("grid.filterResult", { filtered: filteredCount, total: totalCount })}
        </span>
      )}
    </div>
  );
};

export default FilterBar;
