import { useEffect, useState } from "react";
import { listen, EVENTS } from "@/api/event";
import { getExtendViewport } from "@/api/monitorConfig";

export interface ExtendViewport {
  offsetX: number;
  offsetY: number;
  totalWidth: number;
  totalHeight: number;
  myWidth: number;
  myHeight: number;
}

/**
 * useExtendViewport - 订阅 Rust 推送的 extend 裁剪参数
 *
 * M20 起前端不再调 availableMonitors() / getCurrentWindow().scaleFactor()：
 * 壁纸窗口嵌入桌面后拿到的 scaleFactor 是主屏 DPI，多屏异构时必然错位。
 * Rust 用 GetDpiForMonitor 拿目标屏真实 DPI，算好 CSS 像素后按 monitor_id 定向推送。
 *
 * 挂载时主动拉一次，避免 setup 阶段的 init 事件早于 React 订阅发出而丢失。
 */
export function useExtendViewport(monitorId: string | null, displayMode: string) {
  const [extendViewport, setExtendViewport] = useState<ExtendViewport | null>(null);

  useEffect(() => {
    if (displayMode !== "extend" || !monitorId) {
      setExtendViewport(null);
      return;
    }

    let cancelled = false;
    // 先拉当前值：事件可能在订阅前已经发出。
    getExtendViewport(monitorId)
      .then((payload) => {
        if (!cancelled && payload) {
          setExtendViewport({
            offsetX: payload.offsetX,
            offsetY: payload.offsetY,
            totalWidth: payload.totalWidth,
            totalHeight: payload.totalHeight,
            myWidth: payload.monitorWidth,
            myHeight: payload.monitorHeight,
          });
        }
      })
      .catch((e) => console.error("[useExtendViewport] Failed to fetch:", e));

    let unlisten: (() => void) | undefined;
    listen(EVENTS.EXTEND_VIEWPORT, (payload) => {
      if (cancelled) return;
      setExtendViewport({
        offsetX: payload.offsetX,
        offsetY: payload.offsetY,
        totalWidth: payload.totalWidth,
        totalHeight: payload.totalHeight,
        myWidth: payload.monitorWidth,
        myHeight: payload.monitorHeight,
      });
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [monitorId, displayMode]);

  return extendViewport;
}
