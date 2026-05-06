import { useEffect, useRef } from "react";
import { useSearchParams } from "react-router-dom";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useWallpaperLoader } from "@/hooks/useWallpaperLoader";
import { useExtendViewport } from "@/hooks/useExtendViewport";
import { useVideoSync } from "@/hooks/useVideoSync";
import { useInputBlock } from "@/hooks/useInputBlock";
import { useFullscreenPause } from "@/hooks/useFullscreenPause";

/**
 * 壁纸渲染组件 — 壁纸窗口的唯一页面
 * Rust 通过 WebviewWindow(url="/wallpaper?monitorId=xxx") 打开
 *
 * display_mode 支持：
 * - independent: 该显示器独立壁纸，正常渲染
 * - mirror: 与主显示器相同壁纸，正常渲染
 * - extend: 一张壁纸横跨所有显示器，当前窗口只渲染对应区域
 */
const WallpaperRenderer: React.FC = () => {
  const [searchParams] = useSearchParams();
  const monitorId = searchParams.get("monitorId");
  const videoRef = useRef<HTMLVideoElement>(null);

  // 加载壁纸数据 + 监听变更事件
  const { wallpaper, fitMode, displayMode, volume } = useWallpaperLoader(monitorId);

  // 计算 extend 模式视口参数
  const extendViewport = useExtendViewport(monitorId, displayMode);

  // 视频同步（extend + video）
  useVideoSync(videoRef, displayMode, extendViewport, wallpaper?.type);

  // 全屏检测：全屏时暂停 video，退出全屏恢复播放
  useFullscreenPause(videoRef, wallpaper?.type);

  // 禁用所有用户输入事件
  useInputBlock();

  // 壁纸窗口 body 透明
  useEffect(() => {
    document.body.classList.add("wallpaper-body");
    return () => {
      document.body.classList.remove("wallpaper-body");
    };
  }, []);

  // 音量同步到 video 元素
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    video.volume = volume / 100;
    video.muted = volume === 0;
  }, [volume, wallpaper]);

  // ===== 渲染逻辑 =====

  // 无数据时透明
  if (!wallpaper) {
    return (
      <div
        className="h-screen w-screen bg-transparent"
        style={{ pointerEvents: "none", userSelect: "none" }}
      />
    );
  }

  const src = convertFileSrc(wallpaper.file_path);

  // extend 模式：裁剪渲染
  if (displayMode === "extend" && extendViewport) {
    const { offsetX, offsetY, totalWidth, totalHeight } = extendViewport;

    const extendStyle: React.CSSProperties = {
      position: "absolute",
      left: `-${offsetX}px`,
      top: `-${offsetY}px`,
      width: `${totalWidth}px`,
      height: `${totalHeight}px`,
      objectFit: "fill" as const,
    };

    return (
      <div
        className="h-screen w-screen overflow-hidden bg-transparent"
        style={{ position: "relative", pointerEvents: "none", userSelect: "none" }}
      >
        <div style={extendStyle}>
          {wallpaper.type === "video" ? (
            <video
              ref={videoRef}
              src={src}
              autoPlay
              loop
              muted={volume === 0}
              playsInline
              style={{ display: "block", width: "100%", height: "100%", objectFit: "fill" }}
            />
          ) : (
            <img
              src={src}
              alt=""
              draggable={false}
              style={{ display: "block", width: "100%", height: "100%", objectFit: "fill" }}
            />
          )}
        </div>
      </div>
    );
  }

  // independent / mirror 模式
  return (
    <div
      className="h-screen w-screen overflow-hidden bg-transparent"
      style={{ pointerEvents: "none", userSelect: "none" }}
    >
      {wallpaper.type === "video" ? (
        <video
          ref={videoRef}
          src={src}
          autoPlay
          loop
          muted={volume === 0}
          playsInline
          className="h-full w-full"
          style={{ objectFit: fitMode as React.CSSProperties["objectFit"] }}
        />
      ) : (
        <img
          src={src}
          alt=""
          className="h-full w-full"
          draggable={false}
          style={{ objectFit: fitMode as React.CSSProperties["objectFit"] }}
        />
      )}
    </div>
  );
};

export default WallpaperRenderer;