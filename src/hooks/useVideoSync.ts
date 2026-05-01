import { useEffect } from "react";
import { emit } from "@tauri-apps/api/event";
import { listen, EVENTS } from "@/api/event";
import type { ExtendViewport } from "./useExtendViewport";

/**
 * useVideoSync - extend 模式下多窗口视频同步
 *
 * 职责：
 * - Master（offsetX === 0）：每 5s 广播 currentTime
 * - Slave（offsetX > 0）：监听同步事件，漂移 > 0.1s 时校准
 */
export function useVideoSync(
  videoRef: React.RefObject<HTMLVideoElement | null>,
  displayMode: string,
  extendViewport: ExtendViewport | null,
  wallpaperType: string | undefined,
) {
  const isMaster = displayMode === "extend" && extendViewport?.offsetX === 0;
  const isSlave = displayMode === "extend" && extendViewport != null && extendViewport.offsetX > 0;

  // Master：每 5s 广播 currentTime
  useEffect(() => {
    if (!isMaster || wallpaperType !== "video") return;

    const interval = setInterval(() => {
      const video = videoRef.current;
      if (video && !video.paused) {
        emit(EVENTS.VIDEO_SYNC, { current_time: video.currentTime });
      }
    }, 5000);

    return () => clearInterval(interval);
  }, [isMaster, wallpaperType, videoRef]);

  // Slave：监听同步事件
  useEffect(() => {
    if (!isSlave || wallpaperType !== "video") return;

    const unlisten = listen(EVENTS.VIDEO_SYNC, (payload) => {
      const video = videoRef.current;
      if (!video) return;

      const drift = Math.abs(video.currentTime - payload.current_time);
      if (drift > 0.1) {
        video.currentTime = payload.current_time;
      }
    });

    return () => { unlisten.then((fn) => fn()); };
  }, [isSlave, wallpaperType, videoRef]);
}
