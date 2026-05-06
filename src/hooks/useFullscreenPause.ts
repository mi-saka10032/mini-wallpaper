import { useEffect, useRef, useState } from "react";
import { listen, EVENTS } from "@/api/event";

/**
 * useFullscreenPause - 全屏检测暂停 video 壁纸
 *
 * 监听后端 fullscreen-changed 事件，当检测到全屏应用时暂停 video 播放，
 * 全屏退出后恢复播放。返回 isFullscreen 状态供外部使用。
 *
 * 当轮播切换到新 video 时，外部需根据 isFullscreen 决定是否保持暂停。
 */
export function useFullscreenPause(
  videoRef: React.RefObject<HTMLVideoElement | null>,
  wallpaperType: string | undefined,
) {
  const [isFullscreen, setIsFullscreen] = useState(false);
  // 用 ref 追踪最新的 isFullscreen 状态，避免 effect 闭包陈旧
  const isFullscreenRef = useRef(false);

  // 监听全屏事件
  useEffect(() => {
    const unlisten = listen(EVENTS.FULLSCREEN_CHANGED, (payload) => {
      setIsFullscreen(payload.is_fullscreen);
      isFullscreenRef.current = payload.is_fullscreen;

      const video = videoRef.current;
      if (!video || wallpaperType !== "video") return;

      if (payload.is_fullscreen) {
        video.pause();
      } else {
        video.play().catch(() => {});
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [videoRef, wallpaperType]);

  // 当壁纸切换（src 变化）时，如果当前处于全屏状态，确保新 video 也保持暂停
  useEffect(() => {
    if (!isFullscreenRef.current) return;
    if (wallpaperType !== "video") return;

    const video = videoRef.current;
    if (!video) return;

    // 等待 video 加载后暂停（新 src 加载会自动 play 因为有 autoPlay 属性）
    const handlePlay = () => {
      if (isFullscreenRef.current) {
        video.pause();
      }
    };

    video.addEventListener("play", handlePlay);
    return () => {
      video.removeEventListener("play", handlePlay);
    };
  }, [videoRef, wallpaperType]);

  return { isFullscreen };
}
