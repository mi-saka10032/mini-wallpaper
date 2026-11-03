import { memo, type FC } from "react";
import { Search, SortAsc, Tag as TagIcon, Trash2, Unlink, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import PopConfirm from "@/components/ui/pop-confirm";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import type { SortField, SortOrder } from "@/lib/sort";

interface ManageToolbarProps {
  selectedCount: number;
  keyword: string;
  sortField: SortField;
  sortOrder: SortOrder;
  isCollectionView: boolean;
  /** 待移除项数量（收藏夹视图），>0 时取消需二次确认 */
  pendingRemovalsCount: number;
  /** 待删除项数量（全部壁纸视图），>0 时取消需二次确认 */
  pendingDeletionsCount: number;
  onSelectAll: () => void;
  onClearSelection: () => void;
  onKeywordChange: (value: string) => void;
  onSortFieldChange: (value: SortField) => void;
  onSortOrderToggle: () => void;
  onDeleteSelected: () => void;
  onManageTags: () => void;
  onCancel: () => void;
  onDone: () => void;
  /** 退出或确认删除进行中，挡住完成和取消的连点 */
  submitting?: boolean;
}

/** 管理模式下的操作栏 */
const ManageToolbar: FC<ManageToolbarProps> = memo(
  ({
    selectedCount,
    keyword,
    sortField,
    sortOrder,
    isCollectionView,
    pendingRemovalsCount,
    pendingDeletionsCount,
    onSelectAll,
    onClearSelection,
    onKeywordChange,
    onSortFieldChange,
    onSortOrderToggle,
    onDeleteSelected,
    onManageTags,
    onCancel,
    onDone,
    submitting = false,
  }) => {
    const { t } = useTranslation();

    // 两类待提交操作互斥：收藏夹视图只会产生待移除，全部壁纸视图只会产生待删除
    const pendingCount = isCollectionView ? pendingRemovalsCount : pendingDeletionsCount;

    return (
      <>
        <span className="text-sm text-foreground/60">
          {t("main.selected", { count: selectedCount })}
        </span>
        <button
          type="button"
          onClick={onSelectAll}
          className="text-sm text-primary/80 hover:text-primary hover:underline"
        >
          {t("main.selectAll")}
        </button>
        <button
          type="button"
          onClick={onClearSelection}
          className="text-sm text-primary/80 hover:text-primary hover:underline"
        >
          {t("main.clearSelection")}
        </button>

        {/* 搜索框（管理模式） */}
        <div className="relative ml-2 max-w-44">
          <Search className="absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-foreground/40" />
          <Input
            value={keyword}
            onChange={(e) => onKeywordChange(e.target.value)}
            placeholder={t("grid.searchPlaceholder")}
            className="h-7 pl-7 pr-7 text-xs"
          />
          {keyword && (
            <button
              type="button"
              onClick={() => onKeywordChange("")}
              className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded-sm p-0.5 text-foreground/40 hover:text-foreground"
            >
              <X className="size-3" />
            </button>
          )}
        </div>

        {/* 前端排序（管理模式） */}
        <Select value={sortField} onValueChange={(v) => onSortFieldChange(v as SortField)}>
          <SelectTrigger
            size="sm"
            className="h-7 w-auto gap-1 border-none bg-transparent px-2 text-xs text-foreground/60 shadow-none hover:bg-primary-hover hover:text-foreground"
          >
            <SortAsc className="size-3" />
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="created_at">{t("grid.sortByDate")}</SelectItem>
            <SelectItem value="name">{t("grid.sortByName")}</SelectItem>
            <SelectItem value="file_size">{t("grid.sortBySize")}</SelectItem>
            <SelectItem value="type">{t("grid.sortByType")}</SelectItem>
          </SelectContent>
        </Select>
        <button
          type="button"
          onClick={onSortOrderToggle}
          className={cn(
            "flex size-7 items-center justify-center rounded-md text-foreground/60 transition-colors hover:bg-primary-hover hover:text-foreground",
            sortOrder === "desc" && "rotate-180",
          )}
          title={sortOrder === "asc" ? t("grid.ascending") : t("grid.descending")}
        >
          <SortAsc className="size-3.5" />
        </button>

        <div className="flex-1" />
        {selectedCount > 0 && (
          <button
            type="button"
            onClick={onManageTags}
            className="flex items-center gap-1 rounded-md px-2 py-1 text-sm text-foreground/60 transition-colors hover:bg-primary-hover"
          >
            <TagIcon className="size-3.5" />
            {t("tags.manageTags")}
          </button>
        )}
        {selectedCount > 0 && (
          <button
            type="button"
            onClick={onDeleteSelected}
            className={cn(
              "flex items-center gap-1 rounded-md px-2 py-1 text-sm transition-colors",
              isCollectionView
                ? "text-foreground/60 hover:bg-primary-hover"
                : "text-destructive/80 hover:bg-destructive/8 hover:text-destructive",
            )}
          >
            {isCollectionView ? (
              <>
                <Unlink className="size-3.5" />
                {t("main.remove")}
              </>
            ) : (
              <>
                <Trash2 className="size-3.5" />
                {t("main.delete")}
              </>
            )}
          </button>
        )}
        <PopConfirm
          enabled={pendingCount > 0}
          align="end"
          side="bottom"
          title={t("main.cancelConfirmTitle")}
          description={
            isCollectionView
              ? t("main.cancelConfirmRemovals", { count: pendingCount })
              : t("main.cancelConfirmDeletions", { count: pendingCount })
          }
          confirmText={t("main.cancelConfirmDiscard")}
          cancelText={t("main.cancelConfirmKeep")}
          confirmVariant="destructive"
          onConfirm={onCancel}
        >
          <Button variant="ghost" size="sm" disabled={submitting}>
            {t("main.cancel")}
          </Button>
        </PopConfirm>
        <Button variant="outline" size="sm" onClick={onDone} disabled={submitting}>
          {t("main.done")}
        </Button>
      </>
    );
  },
);

ManageToolbar.displayName = "ManageToolbar";

export default ManageToolbar;
