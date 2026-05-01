import { useVirtualizer } from "@tanstack/react-virtual";
import React, { useEffect, useMemo, useRef, useState } from "react";

// ============ 断点配置 ============

/** 列数断点：与 Tailwind 的 grid-cols-3 xl:grid-cols-4 2xl:grid-cols-5 对应 */
const BREAKPOINTS = [
  { minWidth: 1536, cols: 5 }, // 2xl
  { minWidth: 1280, cols: 4 }, // xl
  { minWidth: 0, cols: 3 },    // 默认
] as const;

/** 网格间距（与 gap-3 = 12px 对应） */
const GAP = 12;

/** 虚拟滚动启用阈值 */
const VIRTUAL_THRESHOLD = 100;

// ============ 工具函数 ============

/** 根据容器宽度计算列数 */
function getColumnCount(containerWidth: number): number {
  for (const bp of BREAKPOINTS) {
    if (containerWidth >= bp.minWidth) return bp.cols;
  }
  return 3;
}

/** 根据容器宽度和列数计算单个卡片高度（aspect-video 16:9 + 底部信息栏约 30px） */
function estimateRowHeight(containerWidth: number, cols: number): number {
  const cardWidth = (containerWidth - GAP * (cols - 1)) / cols;
  const imageHeight = cardWidth * (9 / 16); // aspect-video
  const infoBarHeight = 30; // px-2 py-1.5 + text
  const borderAndRounding = 2; // border
  return imageHeight + infoBarHeight + borderAndRounding;
}

// ============ 类型定义 ============

export interface VirtualGridProps<T> {
  /** 数据源 */
  items: T[];
  /** 获取唯一 key */
  getKey: (item: T) => string | number;
  /** 渲染单个卡片 */
  renderItem: (item: T, index: number) => React.ReactNode;
  /** 是否强制禁用虚拟滚动（如排序模式） */
  forceDisable?: boolean;
  /** 额外的尾部元素（如导入卡片） */
  trailingElement?: React.ReactNode;
  /** 容器 className */
  className?: string;
}

// ============ 虚拟行组件（memo 化避免滚动时不必要的重渲染） ============

interface VirtualRowProps<T> {
  rowItems: T[];
  cols: number;
  rowIndex: number;
  getKey: (item: T) => string | number;
  renderItem: (item: T, index: number) => React.ReactNode;
  style: React.CSSProperties;
  gap: number;
}

function VirtualRowInner<T>({
  rowItems,
  cols,
  rowIndex,
  getKey,
  renderItem,
  style,
  gap,
}: VirtualRowProps<T>) {
  return (
    <div style={style}>
      <div
        className="grid"
        style={{
          gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))`,
          gap: `${gap}px`,
        }}
      >
        {rowItems.map((item, colIndex) => {
          const originalIndex = rowIndex * cols + colIndex;
          return (
            <div key={getKey(item)}>{renderItem(item, originalIndex)}</div>
          );
        })}
      </div>
    </div>
  );
}

/** 自定义比较函数：避免 rowItems 因 slice 产生新引用而导致 memo 失效 */
function virtualRowAreEqual<T>(prev: VirtualRowProps<T>, next: VirtualRowProps<T>): boolean {
  // 注：renderItem 和 getKey 通过稳定引用传入，无需比较
  if (
    prev.cols !== next.cols ||
    prev.rowIndex !== next.rowIndex ||
    prev.gap !== next.gap
  ) {
    return false;
  }
  // 比较 style 对象的关键属性
  const ps = prev.style as Record<string, unknown>;
  const ns = next.style as Record<string, unknown>;
  if (ps.height !== ns.height || ps.transform !== ns.transform) {
    return false;
  }
  // 比较 rowItems：长度相同且每项 key 相同即认为相等
  if (prev.rowItems.length !== next.rowItems.length) return false;
  for (let i = 0; i < prev.rowItems.length; i++) {
    if (prev.getKey(prev.rowItems[i]) !== next.getKey(next.rowItems[i])) {
      return false;
    }
  }
  return true;
}

const VirtualRow = React.memo(VirtualRowInner, virtualRowAreEqual) as typeof VirtualRowInner;

// ============ VirtualGrid 组件 ============

function VirtualGrid<T>({
  items,
  getKey,
  renderItem,
  forceDisable = false,
  trailingElement,
  className,
}: VirtualGridProps<T>) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [containerWidth, setContainerWidth] = useState(0);

  // 用 ref 持有 renderItem，避免内联函数引用变化导致 VirtualRow memo 失效
  const renderItemRef = useRef(renderItem);
  renderItemRef.current = renderItem;

  // 稳定引用的 renderItem wrapper
  const stableRenderItem = useMemo(
    () => (item: T, index: number) => renderItemRef.current(item, index),
    [],
  );

  // 监听容器宽度变化
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const width = entry.contentRect.width;
        if (width > 0) {
          setContainerWidth(width);
        }
      }
    });

    observer.observe(el);
    // 初始测量
    setContainerWidth(el.clientWidth);

    return () => observer.disconnect();
  }, []);

  const cols = useMemo(() => getColumnCount(containerWidth), [containerWidth]);
  const rowHeight = useMemo(() => estimateRowHeight(containerWidth, cols), [containerWidth, cols]);

  // 将 items 按行分组
  const rows = useMemo(() => {
    const result: T[][] = [];
    for (let i = 0; i < items.length; i += cols) {
      result.push(items.slice(i, i + cols));
    }
    return result;
  }, [items, cols]);

  // 是否启用虚拟滚动
  const enableVirtual = !forceDisable && items.length > VIRTUAL_THRESHOLD;

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => containerRef.current,
    estimateSize: () => rowHeight + GAP,
    overscan: 3,
    enabled: enableVirtual,
  });

  // 当列数或行高变化时，通知 virtualizer 重新测量
  useEffect(() => {
    if (enableVirtual) {
      virtualizer.measure();
    }
  }, [cols, rowHeight, enableVirtual, virtualizer]);

  // ===== 非虚拟模式：直接渲染 =====
  if (!enableVirtual) {
    return (
      <div ref={containerRef} className={className}>
        <div
          className="grid gap-3"
          style={{ gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))` }}
        >
          {items.map((item, index) => (
            <div key={getKey(item)}>{stableRenderItem(item, index)}</div>
          ))}
          {trailingElement}
        </div>
      </div>
    );
  }

  // ===== 虚拟模式 =====
  const virtualRows = virtualizer.getVirtualItems();
  const totalHeight = virtualizer.getTotalSize();

  return (
    <div
      ref={containerRef}
      className={className}
      style={{ overflow: "auto" }}
    >
      <div
        style={{
          height: `${totalHeight}px`,
          width: "100%",
          position: "relative",
        }}
      >
        {virtualRows.map((virtualRow) => {
          const rowItems = rows[virtualRow.index];
          return (
            <VirtualRow
              key={virtualRow.key}
              rowItems={rowItems}
              cols={cols}
              rowIndex={virtualRow.index}
              getKey={getKey}
              renderItem={stableRenderItem}
              gap={GAP}
              style={{
                position: "absolute",
                top: 0,
                left: 0,
                width: "100%",
                height: `${virtualRow.size - GAP}px`,
                transform: `translateY(${virtualRow.start}px)`,
              }}
            />
          );
        })}
      </div>
      {trailingElement && (
        <div className="mt-3">{trailingElement}</div>
      )}
    </div>
  );
}

export { VIRTUAL_THRESHOLD };
export default VirtualGrid;