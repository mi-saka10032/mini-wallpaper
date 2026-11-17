import { Check, Tags, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { TagWithCount } from "@/api/config";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { cn } from "@/lib/utils";

export interface TagSelectProps {
  /** 全部可选标签（带引用计数），空数组时控件禁用 */
  options: TagWithCount[];
  /** 已选标签 id 集合（受控） */
  selectedIds: Set<number>;
  /** 选择变化 */
  onChange: (ids: Set<number>) => void;
  className?: string;
}

/**
 * 标签多选筛选器：Popover + 勾选列表，仅可选择已有标签。
 *
 * 用于空间紧凑的工具栏（如 FilterBar）——TagInput 的 chips 在窄容器里会
 * 换行把工具栏撑高，而 Select 形态宽度恒定。命中任一选中 tag 即算匹配，
 * 求值在后端（get_wallpapers_by_tags），本组件只维护 id 集合。
 */
export const TagSelect: React.FC<TagSelectProps> = ({
  options,
  selectedIds,
  onChange,
  className,
}) => {
  const { t } = useTranslation();

  const toggle = (id: number) => {
    const next = new Set(selectedIds);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    onChange(next);
  };

  const hasTags = options.length > 0;

  return (
    <div className={cn("relative shrink-0", className)}>
      <Popover>
        <PopoverTrigger asChild>
          <button
            type="button"
            disabled={!hasTags}
            title={hasTags ? t("grid.filterByTag") : t("grid.noTagsYet")}
            className={cn(
              "flex h-8 w-full items-center gap-1.5 rounded-md border border-border/50 px-2 text-xs text-muted-foreground transition-colors hover:bg-primary-hover hover:text-foreground disabled:pointer-events-none disabled:opacity-50",
              selectedIds.size > 0 && "border-primary/40 pr-6 text-primary",
            )}
          >
            <Tags className="size-3.5 shrink-0" />
            <span className="min-w-0 flex-1 truncate text-left">
              {selectedIds.size > 0
                ? t("grid.tagFilterActive", { count: selectedIds.size })
                : t("grid.filterByTag")}
            </span>
          </button>
        </PopoverTrigger>
        <PopoverContent align="start" className="max-h-72 w-56 overflow-y-auto p-1">
          {options.map((tagItem) => {
            const checked = selectedIds.has(tagItem.id);
            return (
              <button
                key={tagItem.id}
                type="button"
                onClick={() => toggle(tagItem.id)}
                className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-sm hover:bg-accent hover:text-accent-foreground"
              >
                <span
                  className={cn(
                    "flex size-4 shrink-0 items-center justify-center rounded border",
                    checked ? "border-primary bg-primary text-primary-foreground" : "border-input",
                  )}
                >
                  {checked && <Check className="size-3" />}
                </span>
                <span className="min-w-0 flex-1 truncate text-left">{tagItem.name}</span>
                <span className="shrink-0 text-xs text-foreground/40">
                  {tagItem.wallpaperCount}
                </span>
              </button>
            );
          })}
        </PopoverContent>
      </Popover>
      {hasTags && selectedIds.size > 0 && (
        <button
          type="button"
          onClick={() => onChange(new Set())}
          title={t("grid.clearTagFilter")}
          className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded-sm p-0.5 text-foreground/40 hover:text-foreground"
        >
          <X className="size-3" />
        </button>
      )}
    </div>
  );
};

export default TagSelect;
