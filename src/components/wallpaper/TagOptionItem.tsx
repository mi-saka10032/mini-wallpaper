import { Check } from "lucide-react";

import { cn } from "@/lib/utils";

export interface TagOptionItemProps {
  label: string;
  checked: boolean;
  onToggle: () => void;
  /** 引用计数，仅 TagSelect 展示 */
  count?: number;
  /** TagInput 用来 preventDefault 保住输入框焦点，支持连续勾选 */
  onMouseDown?: (e: React.MouseEvent<HTMLButtonElement>) => void;
}

/**
 * 标签下拉选项：带 checkbox 的可勾选 option。
 *
 * TagInput 与 TagSelect 共用这一份视觉与勾选语义：选中项不会从列表消失，
 * 再点一次即取消勾选。两个组件的差异只在「能否输入创建新标签」——
 * TagSelect 纯选择，TagInput 支持输入后回车生成新的可勾选 option。
 */
export const TagOptionItem: React.FC<TagOptionItemProps> = ({
  label,
  checked,
  onToggle,
  count,
  onMouseDown,
}) => (
  <button
    type="button"
    aria-pressed={checked}
    onMouseDown={onMouseDown}
    onClick={onToggle}
    className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm hover:bg-accent hover:text-accent-foreground"
  >
    <span
      className={cn(
        "flex size-4 shrink-0 items-center justify-center rounded border",
        checked ? "border-primary bg-primary text-primary-foreground" : "border-input",
      )}
    >
      {checked && <Check className="size-3" />}
    </span>
    <span className="min-w-0 flex-1 truncate">{label}</span>
    {count !== undefined && <span className="shrink-0 text-xs text-foreground/40">{count}</span>}
  </button>
);

export default TagOptionItem;
