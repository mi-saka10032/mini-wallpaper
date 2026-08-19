import { X } from "lucide-react";
import { useCallback, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";

// ============ 类型定义 ============

export interface TagInputProps {
  /** 当前已选标签名集合（受控） */
  value: string[];
  /** 变更回调（新集合已去重、trim） */
  onChange: (next: string[]) => void;
  /** 建议列表（全部已有标签名，用于下拉联想） */
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
 * 可创建标签输入（creatable chips）
 *
 * - 自由输入：回车 / 逗号确认当前输入为一个标签 chip
 * - 退格：输入为空时删除最后一个 chip
 * - 联想：基于 suggestions 过滤未选中的候选，点击即添加
 * - 去重：内部对 value 做规范化，重复名不会重复添加
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
  const [focused, setFocused] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  const tags = useMemo(() => normalize(value), [value]);

  const addTag = useCallback(
    (name: string) => {
      const trimmed = name.trim();
      if (!trimmed) return;
      if (tags.includes(trimmed)) {
        setInput("");
        return;
      }
      onChange(normalize([...tags, trimmed]));
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

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (e.key === "Enter" || e.key === ",") {
        e.preventDefault();
        addTag(input);
      } else if (e.key === "Backspace" && input === "" && tags.length > 0) {
        removeTag(tags[tags.length - 1]);
      }
    },
    [input, tags, addTag, removeTag],
  );

  // 联想候选：未选中 + 匹配当前输入（大小写不敏感），最多 8 条
  const filteredSuggestions = useMemo(() => {
    const kw = input.trim().toLowerCase();
    return suggestions
      .filter((s) => !tags.includes(s))
      .filter((s) => (kw ? s.toLowerCase().includes(kw) : true))
      .slice(0, 8);
  }, [suggestions, tags, input]);

  const showSuggestions = focused && filteredSuggestions.length > 0;

  return (
    <div className={cn("relative", className)}>
      <div
        className={cn(
          "flex min-h-9 w-full flex-wrap items-center gap-1.5 rounded-md border border-input bg-transparent px-2 py-1.5 text-sm transition-colors",
          focused && "border-ring ring-2 ring-ring/20",
          disabled && "pointer-events-none opacity-50",
        )}
        onClick={() => inputRef.current?.focus()}
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
          onFocus={() => setFocused(true)}
          onBlur={() => {
            // 延迟收起，保证点击建议项能先触发
            setTimeout(() => setFocused(false), 120);
            addTag(input);
          }}
          placeholder={tags.length === 0 ? (placeholder ?? t("tags.inputPlaceholder")) : ""}
          className="min-w-24 flex-1 bg-transparent text-sm outline-none placeholder:text-foreground/40"
        />
      </div>

      {/* 联想候选下拉 */}
      {showSuggestions && (
        <div className="absolute z-50 mt-1 max-h-48 w-full overflow-y-auto rounded-md border bg-popover p-1 shadow-md">
          {filteredSuggestions.map((s) => (
            <button
              key={s}
              type="button"
              onMouseDown={(e) => {
                // mousedown 早于 input blur，避免下拉先关闭
                e.preventDefault();
                addTag(s);
              }}
              className="flex w-full items-center rounded-sm px-2 py-1.5 text-left text-sm hover:bg-accent hover:text-accent-foreground"
            >
              <span className="truncate">{s}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
};

export default TagInput;
