import { useCallback, useEffect, useState } from "react";
import { availableMonitors, getCurrentWindow } from "@tauri-apps/api/window";

export interface ExtendViewport {
  offsetX: number;
  offsetY: number;
  totalWidth: number;
  totalHeight: number;
  myWidth: number;
  myHeight: number;
}

/**
 * useExtendViewport - 计算 extend 模式下当前显示器的视口裁剪参数
 *
 * 职责：
 * - 当 displayMode === "extend" 时，通过 Tauri availableMonitors() 计算视口
 * - 返回 extendViewport（含 offsetX/Y、totalWidth/Height、myWidth/Height）
 */
export function useExtendViewport(monitorId: string | null, displayMode: string) {
  const [extendViewport, setExtendViewport] = useState<ExtendViewport | null>(null);

  const computeExtendViewport = useCallback(async () => {
    if (!monitorId) return;

    try {
      const monitors = await availableMonitors();
      if (monitors.length === 0) return;

      const scaleFactor = await getCurrentWindow().scaleFactor();

      let minX = Infinity, minY = Infinity;
      let maxX = -Infinity, maxY = -Infinity;

      for (const m of monitors) {
        const x = m.position.x;
        const y = m.position.y;
        const w = m.size.width;
        const h = m.size.height;
        minX = Math.min(minX, x);
        minY = Math.min(minY, y);
        maxX = Math.max(maxX, x + w);
        maxY = Math.max(maxY, y + h);
      }

      const totalWidth = (maxX - minX) / scaleFactor;
      const totalHeight = (maxY - minY) / scaleFactor;

      const currentMonitor = monitors.find(
        (m) => (m.name ?? `monitor_${monitors.indexOf(m)}`) === monitorId
      );

      if (!currentMonitor) {
        console.warn("[useExtendViewport] Monitor not found:", monitorId);
        return;
      }

      const offsetX = (currentMonitor.position.x - minX) / scaleFactor;
      const offsetY = (currentMonitor.position.y - minY) / scaleFactor;
      const myWidth = currentMonitor.size.width / scaleFactor;
      const myHeight = currentMonitor.size.height / scaleFactor;

      console.log("[useExtendViewport] viewport computed:", {
        scaleFactor,
        totalWidth, totalHeight,
        offsetX, offsetY,
        myWidth, myHeight,
        rawPhysical: { totalW: maxX - minX, totalH: maxY - minY },
      });

      setExtendViewport({ offsetX, offsetY, totalWidth, totalHeight, myWidth, myHeight });
    } catch (e) {
      console.error("[useExtendViewport] Failed to compute:", e);
    }
  }, [monitorId]);

  useEffect(() => {
    if (displayMode === "extend") {
      computeExtendViewport();
    } else {
      setExtendViewport(null);
    }
  }, [displayMode, computeExtendViewport]);

  return extendViewport;
}
