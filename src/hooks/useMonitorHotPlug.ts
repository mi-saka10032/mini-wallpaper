import { useEffect, useRef } from "react";
import { availableMonitors } from "@tauri-apps/api/window";
import { useMonitorConfigStore } from "@/stores/monitorConfigStore";

/** 轮询间隔（ms） */
const POLL_INTERVAL = 5000;

/**
 * 显示器热插拔检测 hook
 *
 * 每 5s 轮询 availableMonitors()，比对显示器 name 集合是否变化，
 * 变化时自动调用 syncMonitors() 刷新配置。
 */
export function useMonitorHotPlug() {
  const syncMonitors = useMonitorConfigStore((s) => s.syncMonitors);
  const lastSnapshotRef = useRef<string>("");

  useEffect(() => {
    let active = true;

    const poll = async () => {
      try {
        const monitors = await availableMonitors();
        const snapshot = monitors
          .map((m) => m.name ?? `monitor_${monitors.indexOf(m)}`)
          .sort()
          .join(",");

        if (lastSnapshotRef.current && snapshot !== lastSnapshotRef.current) {
          console.log("[HotPlug] Display change detected:", snapshot);
          await syncMonitors();
        }

        lastSnapshotRef.current = snapshot;
      } catch (e) {
        console.error("[HotPlug] Poll error:", e);
      }
    };

    // 首次立即获取快照
    poll();

    const timer = setInterval(() => {
      if (active) poll();
    }, POLL_INTERVAL);

    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [syncMonitors]);
}
