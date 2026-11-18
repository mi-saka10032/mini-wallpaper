import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

/**
 * 拖拽文件导入 Hook（双通道）
 *
 * ## 为何需要两条通道
 *
 * 原先只用 HTML5 DnD（`dataTransfer.files` → `File.arrayBuffer()` → 字节直传后端）。
 * 该方案在 `pnpm tauri dev` 下正常，但生产构建后拖入文件只闪一下 loading、
 * 壁纸并未入库 —— WebView2 在 `tauri://` 自定义协议下给出的 File 对象
 * `size` 正常却读不出内容，`arrayBuffer()` 返回空，后端自然拿不到数据。
 *
 * 因此改为优先走 Tauri 原生拖放事件（`onDragDropEvent`），它直接给出文件系统
 * 路径，由 Rust 侧做文件级 copy，与「点击导入」完全同一条已验证链路。
 *
 * HTML5 通道保留作为降级：纯浏览器环境（非 Tauri 容器）下只有它会触发。
 *
 * ## 已知平台差异（不是 bug，无需修）
 *
 * Windows 上 `pnpm tauri dev` 期间原生拖放**不可用**（光标为禁止态），
 * release 构建却正常。原因是 wry 用 `EnumChildWindows` 遍历子窗口注入
 * `IDropTarget`（wry/src/webview2/drag_drop.rs），而 WebView2 的子 HWND
 * 是异步创建的：dev 下页面要等 Vite 服务器与 HMR 握手，注入时子窗口尚未就绪，
 * 遍历扑空；release 从内嵌 dist 加载，速度快，注入成功。属竞态，
 * 与 `dragDropEnabled` 无关。验证拖拽功能请用 release 构建。
 *
 * ## 其它设计要点
 * - HTML5 侧仅当 `dataTransfer.types.includes("Files")` 时响应，
 *   自动屏蔽组件内部元素（如 @dnd-kit）的拖拽事件，避免误触发。
 *   （@dnd-kit 用的是 PointerSensor，本就不依赖 HTML5 DnD。）
 * - 用 `dragCounterRef` 处理嵌套元素的 enter/leave 事件抖动。
 * - `enabled = false` 时所有 handler 为 noop，浏览器自动显示禁止光标。
 *
 * @param enabled    是否启用拖拽响应（管理模式 / 排序模式 / 收藏夹下传 false）
 * @param onImport   HTML5 通道回调，拿到 File[]（降级路径）
 * @param onImportPaths 原生通道回调，拿到文件路径（Tauri 容器内的主路径）
 */
export function useDropImport({
  enabled,
  onImport,
  onImportPaths,
}: {
  enabled: boolean;
  onImport: (files: File[]) => void | Promise<void>;
  onImportPaths?: (paths: string[]) => void | Promise<void>;
}) {
  const [isDragOver, setIsDragOver] = useState(false);
  const dragCounterRef = useRef(0);

  // 用 ref 持有回调，避免其引用变化导致原生监听反复解绑重绑
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;
  const onImportPathsRef = useRef(onImportPaths);
  onImportPathsRef.current = onImportPaths;

  // 原生拖放通道：仅在 Tauri 容器内生效
  useEffect(() => {
    if (!onImportPathsRef.current) return;
    if (!("__TAURI_INTERNALS__" in window)) return;

    let unlisten: (() => void) | undefined;
    let disposed = false;

    getCurrentWebviewWindow()
      .onDragDropEvent((event) => {
        // enabled 通过 ref 读取：禁用期间仍需消费事件以维持蒙层状态正确，
        // 只是不触发导入
        const payload = event.payload;
        if (payload.type === "enter") {
          if (enabledRef.current) setIsDragOver(true);
        } else if (payload.type === "leave") {
          setIsDragOver(false);
        } else if (payload.type === "drop") {
          setIsDragOver(false);
          if (!enabledRef.current) return;
          if (payload.paths.length === 0) return;
          void Promise.resolve(onImportPathsRef.current?.(payload.paths)).catch(() => {});
        }
      })
      .then((fn) => {
        // 组件可能在 Promise resolve 前卸载
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch((e) => console.error("[useDropImport] onDragDropEvent failed", e));

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

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

      // Tauri 容器内由原生通道负责，避免同一次拖放被处理两次
      if ("__TAURI_INTERNALS__" in window) return;

      const files = Array.from(e.dataTransfer.files);
      if (files.length === 0) return;
      void Promise.resolve(onImport(files)).catch(() => {});
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
