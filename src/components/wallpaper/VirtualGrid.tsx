import { useVirtualizer } from "@tanstack/react-virtual";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

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

  // 计算原始 index
  const getOriginalIndex = useCallback(
    (rowIndex: number, colIndex: number) => rowIndex * cols + colIndex,
    [cols],
  );

  // ===== 非虚拟模式：直接渲染 =====
  if (!enableVirtual) {
    return (
      <div ref={containerRef} className={className}>
        <div
          className="grid gap-3"
          style={{ gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))` }}
        >
          {items.map((item, index) => (
            <div key={getKey(item)}>{renderItem(item, index)}</div>
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
            <div
              key={virtualRow.key}
              style={{
                position: "absolute",
                top: 0,
                left: 0,
                width: "100%",
                height: `${virtualRow.size - GAP}px`,
                transform: `translateY(${virtualRow.start}px)`,
              }}
            >
              <div
                className="grid gap-3"
                style={{ gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))` }}
              >
                {rowItems.map((item, colIndex) => {
                  const originalIndex = getOriginalIndex(virtualRow.index, colIndex);
                  return (
                    <div key={getKey(item)}>{renderItem(item, originalIndex)}</div>
                  );
                })}
              </div>
            </div>
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
