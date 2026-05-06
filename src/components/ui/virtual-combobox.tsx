import * as React from "react";
import { useCallback, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { CheckIcon, ChevronDownIcon, Search } from "lucide-react";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { cn } from "@/lib/utils";

/** 单个选项的数据结构 */
export interface ComboboxOption {
  value: string;
  label: string;
  /** 可选：自定义渲染内容 */
  render?: React.ReactNode;
  /** 可选：用于搜索匹配的额外关键词 */
  searchKeywords?: string;
}

export interface VirtualComboboxProps {
  /** 选项列表 */
  options: ComboboxOption[];
  /** 当前选中值 */
  value: string;
  /** 值变更回调 */
  onValueChange: (value: string) => void;
  /** 占位文本 */
  placeholder?: string;
  /** 搜索框占位文本 */
  searchPlaceholder?: string;
  /** 无结果时的提示文本 */
  emptyText?: string;
  /** 下拉面板最大高度（px），默认 320 */
  maxHeight?: number;
  /** 单项高度估算（px），默认 36 */
  itemHeight?: number;
  /** 虚拟滚动启用阈值（选项数量），默认 50 */
  virtualThreshold?: number;
  /** 是否显示搜索框，默认 true */
  showSearch?: boolean;
  /** 触发器 className */
  triggerClassName?: string;
  /** 弹出层 className */
  contentClassName?: string;
  /** 是否禁用 */
  disabled?: boolean;
}

/** 虚拟滚动 Combobox 组件 */
function VirtualCombobox({
  options,
  value,
  onValueChange,
  placeholder = "Select...",
  searchPlaceholder = "Search...",
  emptyText = "No results",
  maxHeight = 320,
  itemHeight = 36,
  virtualThreshold = 50,
  showSearch = true,
  triggerClassName,
  contentClassName,
  disabled = false,
}: VirtualComboboxProps) {
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // 过滤后的选项
  const filteredOptions = useMemo(() => {
    if (!search.trim()) return options;
    const kw = search.trim().toLowerCase();
    return options.filter((opt) => {
      const labelMatch = opt.label.toLowerCase().includes(kw);
      const keywordMatch = opt.searchKeywords?.toLowerCase().includes(kw);
      return labelMatch || keywordMatch;
    });
  }, [options, search]);

  // 是否启用虚拟滚动
  const enableVirtual = filteredOptions.length > virtualThreshold;

  // 虚拟化
  const virtualizer = useVirtualizer({
    count: filteredOptions.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => itemHeight,
    overscan: 8,
    enabled: enableVirtual,
  });

  // 选中项的 render（用于 trigger 显示）
  const selectedRender = useMemo(() => {
    const found = options.find((opt) => opt.value === value);
    return found?.render ?? found?.label ?? null;
  }, [options, value]);

  const handleSelect = useCallback(
    (optValue: string) => {
      onValueChange(optValue);
      setOpen(false);
      setSearch("");
    },
    [onValueChange],
  );

  const handleOpenChange = useCallback((isOpen: boolean) => {
    setOpen(isOpen);
    if (!isOpen) {
      setSearch("");
    }
  }, []);

  // 打开时自动聚焦搜索框
  const handleContentMount = useCallback(() => {
    setTimeout(() => inputRef.current?.focus(), 0);
  }, []);

  // 列表内容高度（非虚拟模式下用于限制 max-height）
  const listMaxHeight = showSearch ? maxHeight - 44 : maxHeight; // 44px 为搜索框区域高度

  return (
    <Popover open={open} onOpenChange={handleOpenChange}>
      <PopoverTrigger asChild disabled={disabled}>
        <button
          type="button"
          role="combobox"
          aria-expanded={open}
          disabled={disabled}
          className={cn(
            "flex h-9 w-full items-center justify-between gap-2 rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs transition-[color,box-shadow] outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50 data-[placeholder]:text-muted-foreground dark:bg-input/30 dark:hover:bg-input/50",
            triggerClassName,
          )}
        >
          <span className="flex min-w-0 flex-1 items-center gap-2 truncate">
            {selectedRender || (
              <span className="text-muted-foreground">{placeholder}</span>
            )}
          </span>
          <ChevronDownIcon className="size-4 shrink-0 opacity-50" />
        </button>
      </PopoverTrigger>

      <PopoverContent
        className={cn("w-[var(--radix-popover-trigger-width)] p-0", contentClassName)}
        align="start"
        sideOffset={4}
        onOpenAutoFocus={handleContentMount}
      >
        {/* 搜索框 */}
        {showSearch && (
          <div className="flex items-center gap-2 border-b border-border/60 px-3 py-2">
            <Search className="size-4 shrink-0 text-muted-foreground" />
            <input
              ref={inputRef}
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={searchPlaceholder}
              className="h-6 w-full bg-transparent text-sm outline-none placeholder:text-muted-foreground"
            />
          </div>
        )}

        {/* 列表区域 */}
        <div
          ref={scrollRef}
          className="overflow-y-auto overflow-x-hidden"
          style={{ maxHeight: `${listMaxHeight}px` }}
        >
          {filteredOptions.length === 0 ? (
            <div className="flex items-center justify-center py-6 text-sm text-muted-foreground">
              {emptyText}
            </div>
          ) : enableVirtual ? (
            /* 虚拟滚动模式 */
            <div
              style={{
                height: `${virtualizer.getTotalSize()}px`,
                width: "100%",
                position: "relative",
              }}
            >
              {virtualizer.getVirtualItems().map((virtualItem) => {
                const opt = filteredOptions[virtualItem.index];
                const isSelected = opt.value === value;
                return (
                  <div
                    key={opt.value}
                    data-index={virtualItem.index}
                    className={cn(
                      "absolute left-0 top-0 flex w-full cursor-default items-center gap-2 rounded-sm px-2 py-1.5 text-sm select-none transition-colors hover:bg-accent hover:text-accent-foreground",
                      isSelected && "bg-accent/50",
                    )}
                    style={{
                      height: `${virtualItem.size}px`,
                      transform: `translateY(${virtualItem.start}px)`,
                    }}
                    onClick={() => handleSelect(opt.value)}
                  >
                    <span className="flex min-w-0 flex-1 items-center gap-2 truncate">
                      {opt.render ?? opt.label}
                    </span>
                    {isSelected && (
                      <CheckIcon className="size-4 shrink-0 text-primary" />
                    )}
                  </div>
                );
              })}
            </div>
          ) : (
            /* 非虚拟模式：直接渲染 */
            <div>
              {filteredOptions.map((opt) => {
                const isSelected = opt.value === value;
                return (
                  <div
                    key={opt.value}
                    className={cn(
                      "flex cursor-default items-center gap-2 rounded-sm px-2 py-1.5 text-sm select-none transition-colors hover:bg-accent hover:text-accent-foreground",
                      isSelected && "bg-accent/50",
                    )}
                    onClick={() => handleSelect(opt.value)}
                  >
                    <span className="flex min-w-0 flex-1 items-center gap-2 truncate">
                      {opt.render ?? opt.label}
                    </span>
                    {isSelected && (
                      <CheckIcon className="size-4 shrink-0 text-primary" />
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}

export { VirtualCombobox };
