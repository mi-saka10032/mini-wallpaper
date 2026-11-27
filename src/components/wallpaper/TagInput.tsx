import { X } from "lucide-react";
import { useCallback, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Popover, PopoverAnchor, PopoverContent } from "@/components/ui/popover";
import { cn } from "@/lib/utils";
import TagOptionItem from "./TagOptionItem";

// ============ 类型定义 ============

export interface TagInputProps {
  /** 当前已选标签名集合（受控） */
  value: string[];
  /** 变更回调（新集合已去重、trim） */
  onChange: (next: string[]) => void;
  /** 建议列表（全部已有标签名）；下拉按此顺序展示，新建标签追加在末尾 */
  suggestions?: string[];
  /** 占位提示 */
  placeholder?: string;
  /** 是否禁用 */
  disabled?: boolean;
  /** 自动聚焦 */
  autoFocus?: boolean;
  className?: string;
}

/** 规范化：trim + 去空 + 去重（保序，大小写敏感按原样保留） */
function normalize(names: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of names) {
    const name = raw.trim();
    if (!name) continue;
    if (seen.has(name)) continue;
    seen.add(name);
    out.push(name);
  }
  return out;
}

/**
 * 可创建标签输入（creatable chips + checkbox 下拉）。
 *
 * 与 TagSelect 共用 TagOptionItem 的勾选语义：下拉里每一项都带勾选框，
 * 选中项不会从列表消失，再点一次即取消勾选；chip 上的叉只是另一种取消方式。
 * 差异点：这里支持自由输入，回车 / 逗号生成的新标签会立刻成为可勾选 option。
 *
 * ## 开关只有 Radix 的 open 一个真相
 * 早期用 focused + onBlur 定时器与 Radix 的 open 双源互相覆盖，出现
 * 「点输入框展开又立刻收起」。现在只有人为点击输入框（或按 ArrowDown）
 * 才置开，关闭统一由 onInteractOutside 裁决；因为用的是 PopoverAnchor
 * 而非 Trigger，需要在该回调里对落在 anchor 内的点击 preventDefault，
 * 否则点输入框也会被判成 outside。autoFocus / Tab 的程序性聚焦不展开，
 * 避免打开 Dialog 就自动弹出下拉。
 *
 * ## 下拉 Portal
 * 由 ui/popover 的 PopoverContent 在有 Dialog 时挂到 DialogContent 内、
 * 否则挂 body：不参与父级 overflow-y-auto 的布局，也不会被 modal Dialog
 * 的 RemoveScroll 吞掉滚轮。
 */
export const TagInput: React.FC<TagInputProps> = ({
  value,
  onChange,
  suggestions = [],
  placeholder,
  disabled = false,
  autoFocus = false,
  className,
}) => {
  const { t } = useTranslation();
  const [input, setInput] = useState("");
  const [open, setOpen] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const anchorRef = useRef<HTMLDivElement>(null);

  const tags = useMemo(() => normalize(value), [value]);

  const addTag = useCallback(
    (name: string) => {
      const trimmed = name.trim();
      if (!trimmed) return;
      if (!tags.includes(trimmed)) {
        onChange(normalize([...tags, trimmed]));
      }
      setInput("");
    },
    [tags, onChange],
  );

  const removeTag = useCallback(
    (name: string) => {
      onChange(tags.filter((tenta) => tenta !== name));
    },
    [tags, onChange],
  );

  /** 下拉勾选 / 取消：与 TagSelect 的 toggle 语义一致 */
  const toggleTag = useCallback(
    (name: string) => {
      const trimmed = name.trim();
      if (!trimmed) return;
      if (tags.includes(trimmed)) {
        onChange(tags.filter((tenta) => tenta !== trimmed));
      } else {
        onChange(normalize([...tags, trimmed]));
      }
    },
    [tags, onChange],
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (e.key === "Enter" || e.key === ",") {
        e.preventDefault();
        addTag(input);
      } else if (e.key === "ArrowDown") {
        // 键盘用户主动展开；打开 Dialog 时的程序性聚焦不展开。
        e.preventDefault();
        setOpen(true);
      } else if (e.key === "Backspace" && input === "" && tags.length > 0) {
        removeTag(tags[tags.length - 1]);
      }
    },
    [input, tags, addTag, removeTag],
  );

  // 以 suggestions 的原始顺序为主，仅把「新建但尚未进入全局建议列表」的标签
  // 追加在末尾；已勾选的 option 保持原位，不重排靠前。
  const options = useMemo(() => {
    const kw = input.trim().toLowerCase();
    return normalize([...suggestions, ...tags]).filter((name) =>
      kw ? name.toLowerCase().includes(kw) : true,
    );
  }, [input, tags, suggestions]);

  const showOptions = open && options.length > 0;

  return (
    <Popover open={showOptions} onOpenChange={setOpen}>
      <PopoverAnchor asChild>
        <div className={cn("relative", className)}>
          <div
            ref={anchorRef}
            className={cn(
              "flex min-h-9 w-full flex-wrap items-center gap-1.5 rounded-md border border-input bg-transparent px-2 py-1.5 text-sm transition-colors",
              open && "border-ring ring-2 ring-ring/20",
              disabled && "pointer-events-none opacity-50",
            )}
            onClick={() => {
              inputRef.current?.focus();
              // 只有人为点击才展开下拉；autoFocus / Tab 聚焦不展开。
              setOpen(true);
            }}
          >
            {tags.map((tag) => (
              <span
                key={tag}
                className="flex items-center gap-1 rounded-full bg-primary/10 py-0.5 pl-2 pr-1 text-xs text-primary"
              >
                <span className="max-w-32 truncate">{tag}</span>
                <button
                  type="button"
                  tabIndex={-1}
                  onClick={(e) => {
                    e.stopPropagation();
                    removeTag(tag);
                  }}
                  className="flex size-3.5 items-center justify-center rounded-full text-primary/60 hover:bg-primary/20 hover:text-primary"
                >
                  <X className="size-2.5" />
                </button>
              </span>
            ))}
            <input
              ref={inputRef}
              value={input}
              autoFocus={autoFocus}
              disabled={disabled}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              onBlur={() => addTag(input)}
              placeholder={tags.length === 0 ? (placeholder ?? t("tags.inputPlaceholder")) : ""}
              className="min-w-24 flex-1 bg-transparent text-sm outline-none placeholder:text-foreground/40"
            />
          </div>
        </div>
      </PopoverAnchor>

      {/* z-[70] 高于 TagEditorDialog 的 z-[60] */}
      <PopoverContent
        align="start"
        side="bottom"
        sideOffset={4}
        onOpenAutoFocus={(e) => e.preventDefault()}
        onCloseAutoFocus={(e) => e.preventDefault()}
        onInteractOutside={(e) => {
          // 没有 PopoverTrigger、只有 Anchor：Radix 不认 anchor，
          // 点输入框 / chips 会被判成 outside，必须显式放行。
          if (anchorRef.current?.contains(e.target as Node)) e.preventDefault();
        }}
        className="z-[70] max-h-48 min-w-40 w-[var(--radix-popover-trigger-width)] overflow-y-auto p-1"
      >
        {options.map((name) => (
          <TagOptionItem
            key={name}
            label={name}
            checked={tags.includes(name)}
            onToggle={() => toggleTag(name)}
            onMouseDown={(e) => {
              // mousedown 早于 input blur：阻止焦点转移，支持连续勾选
              e.preventDefault();
            }}
          />
        ))}
      </PopoverContent>
    </Popover>
  );
};

export default TagInput;
