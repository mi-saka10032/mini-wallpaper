import { useCallback, useMemo, useRef, useState } from "react";

/**
 * H5 拖拽文件导入 Hook
 *
 * 设计要点：
 * - 仅当 `dataTransfer.types.includes("Files")` 时响应，
 *   自动屏蔽组件内部元素（如 @dnd-kit）的拖拽事件，避免误触发。
 * - 用 `dragCounterRef` 处理嵌套元素的 enter/leave 事件抖动。
 * - `enabled = false` 时所有 handler 为 noop，浏览器自动显示禁止光标。
 *
 * @param enabled  是否启用拖拽响应（管理模式 / 排序模式 / 收藏夹下传 false）
 * @param onImport 拖入文件后的回调，外部决定如何处理 File[]
 */
export function useDropImport({
  enabled,
  onImport,
}: {
  enabled: boolean;
  onImport: (files: File[]) => void | Promise<void>;
}) {
  const [isDragOver, setIsDragOver] = useState(false);
  const dragCounterRef = useRef(0);

  const dragHandlers = useMemo(() => {
    if (!enabled) {
      // 禁用时，全部 noop，不 preventDefault → 浏览器显示禁止光标
      return {
        onDragEnter: undefined,
        onDragOver: undefined,
        onDragLeave: undefined,
        onDrop: undefined,
      };
    }

    const handleDragEnter = (e: React.DragEvent) => {
      // 仅响应外部文件拖入（types 中包含 "Files"）；
      // 应用内部 @dnd-kit / DOM 拖拽不会带 "Files"，自动忽略
      if (!e.dataTransfer.types.includes("Files")) return;
      e.preventDefault();
      e.stopPropagation();
      dragCounterRef.current += 1;
      setIsDragOver(true);
    };

    const handleDragOver = (e: React.DragEvent) => {
      if (!e.dataTransfer.types.includes("Files")) return;
      e.preventDefault();
      e.stopPropagation();
      e.dataTransfer.dropEffect = "copy";
    };

    const handleDragLeave = (e: React.DragEvent) => {
      if (!e.dataTransfer.types.includes("Files")) return;
      e.preventDefault();
      e.stopPropagation();
      dragCounterRef.current -= 1;
      if (dragCounterRef.current <= 0) {
        dragCounterRef.current = 0;
        setIsDragOver(false);
      }
    };

    const handleDrop = (e: React.DragEvent) => {
      if (!e.dataTransfer.types.includes("Files")) return;
      e.preventDefault();
      e.stopPropagation();
      dragCounterRef.current = 0;
      setIsDragOver(false);

      const files = Array.from(e.dataTransfer.files);
      if (files.length === 0) return;
      void onImport(files);
    };

    return {
      onDragEnter: handleDragEnter,
      onDragOver: handleDragOver,
      onDragLeave: handleDragLeave,
      onDrop: handleDrop,
    };
  }, [enabled, onImport]);

  // 状态切到禁用时，确保覆盖蒙层不残留
  const reset = useCallback(() => {
    dragCounterRef.current = 0;
    setIsDragOver(false);
  }, []);

  return { isDragOver, dragHandlers, reset };
}
