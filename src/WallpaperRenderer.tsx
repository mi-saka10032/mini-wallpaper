import { useCallback, useEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { convertFileSrc } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { listen as tauriListen } from "@tauri-apps/api/event";
import { invoke } from "@/api/invoke";
import { listen, EVENTS } from "@/api/event";
import { COMMANDS, type Wallpaper, type MonitorConfig } from "@/api/config";

/**
 * 壁纸渲染组件 — 壁纸窗口的唯一页面
 * Rust 通过 WebviewWindow(url="/wallpaper?monitorId=xxx") 打开
 *
 * display_mode 支持：
 * - independent: 该显示器独立壁纸，正常渲染
 * - mirror: 与主显示器相同壁纸，正常渲染
 * - extend: 一张壁纸横跨所有显示器，当前窗口只渲染对应区域
 *
 * 视频同步（extend + video）：
 * - 第一个窗口（offsetX=0）作为 master，每 5s emit VIDEO_SYNC 事件广播 currentTime
 * - 其他窗口作为 slave，监听事件并对齐 currentTime（漂移 > 0.1s 时校准）
 *
 * 输入事件处理：
 * - 壁纸窗口是纯展示层，不需要任何用户交互
 * - 前端禁用所有鼠标/键盘事件 + 后端 WS_EX_TRANSPARENT 穿透，双层保障
 */
const WallpaperRenderer: React.FC = () => {
  const [searchParams] = useSearchParams();
  const monitorId = searchParams.get("monitorId");

  const [wallpaper, setWallpaper] = useState<Wallpaper | null>(null);
  const [fitMode, setFitMode] = useState<string>("cover");
  const [displayMode, setDisplayMode] = useState<string>("independent");
  const [extendViewport, setExtendViewport] = useState<{
    offsetX: number;
    totalWidth: number;
    myWidth: number;
  } | null>(null);

  // ===== NC offset 补偿状态（方案B） =====
  // 后端检测到 Windows NC 非客户区偏移后，通过事件传递给前端
  // 前端通过 CSS 负 margin + 放大尺寸来补偿，使内容完全覆盖显示器
  const [ncOffset, setNcOffset] = useState<{
    left: number;
    top: number;
    right: number;
    bottom: number;
  } | null>(null);

  const videoRef = useRef<HTMLVideoElement>(null);

  // ===== 监听 NC offset 事件（方案B核心） =====
  useEffect(() => {
    const unlisten = tauriListen<{
      left: number;
      top: number;
      right: number;
      bottom: number;
    }>("nc-offset-detected", (event) => {
      const offset = event.payload;
      console.log("[WallpaperRenderer] NC offset received:", offset);
      if (offset.left !== 0 || offset.top !== 0 || offset.right !== 0 || offset.bottom !== 0) {
        setNcOffset(offset);
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  // ===== 计算 NC offset 补偿样式 =====
  // 原理：窗口有 NC 边框导致客户区比窗口小，
  // 我们让内容容器用负 margin 向外扩展，填满整个窗口区域（包括 NC 区域）
  // 同时放大宽高以覆盖 NC 边框占用的空间
  const ncCompensationStyle: React.CSSProperties = ncOffset
    ? {
        marginLeft: `-${ncOffset.left}px`,
        marginTop: `-${ncOffset.top}px`,
        width: `calc(100vw + ${ncOffset.left + ncOffset.right}px)`,
        height: `calc(100vh + ${ncOffset.top + ncOffset.bottom}px)`,
        overflow: "hidden",
      }
    : {};

  // ===== 禁用所有用户输入事件 =====
  // 壁纸窗口是纯展示层，不需要任何交互，直接一刀切禁用全部鼠标和键盘事件
  useEffect(() => {
    const blockAll = (e: Event) => {
      e.preventDefault();
      e.stopPropagation();
    };

    // 鼠标类事件
    const mouseEvents = [
      "contextmenu", "click", "dblclick", "mousedown", "mouseup",
      "mousemove", "mouseover", "mouseout", "mouseenter", "mouseleave",
      "wheel", "auxclick",
    ];
    // 键盘类事件
    const keyEvents = ["keydown", "keyup", "keypress"];
    // 拖拽类事件
    const dragEvents = ["dragover", "dragenter", "dragleave", "drop", "drag", "dragstart", "dragend"];
    // 其他交互事件
    const otherEvents = ["selectstart", "copy", "cut", "paste", "focus", "blur"];

    const allEvents = [...mouseEvents, ...keyEvents, ...dragEvents, ...otherEvents];

    for (const evt of allEvents) {
      document.addEventListener(evt, blockAll, true);
    }

    return () => {
      for (const evt of allEvents) {
        document.removeEventListener(evt, blockAll, true);
      }
    };
  }, []);

  // 根据 config 获取壁纸并更新状态
  const loadFromConfig = useCallback(async (config: MonitorConfig) => {
    setFitMode(config.fit_mode || "cover");
    setDisplayMode(config.display_mode || "independent");

    if (!config.wallpaper_id) {
      setWallpaper(null);
      return;
    }

    try {
      const all = await invoke(COMMANDS.GET_WALLPAPERS);
      const found = all.find((w) => w.id === config.wallpaper_id) ?? null;
      setWallpaper(found);
    } catch (e) {
      console.error("[WallpaperRenderer] fetch wallpaper failed:", e);
    }
  }, []);

  // 初始化
  useEffect(() => {
    if (!monitorId) return;
    invoke(COMMANDS.GET_MONITOR_CONFIG, { monitorId }).then((config) => {
      if (config) loadFromConfig(config);
    });
  }, [monitorId, loadFromConfig]);

  // 监听壁纸切换事件
  useEffect(() => {
    if (!monitorId) return;
    const unlisten = listen(EVENTS.WALLPAPER_CHANGED, (payload) => {
      if (payload.monitor_id === monitorId) {
        invoke(COMMANDS.GET_MONITOR_CONFIG, { monitorId }).then((config) => {
          if (config) loadFromConfig(config);
        });
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [monitorId, loadFromConfig]);

  // extend 模式：解析视口参数
  useEffect(() => {
    if (displayMode !== "extend") {
      setExtendViewport(null);
      return;
    }
    const offsetX = parseFloat(searchParams.get("extendOffsetX") ?? "0");
    const totalWidth = parseFloat(searchParams.get("extendTotalWidth") ?? "1");
    const myWidth = parseFloat(searchParams.get("extendMyWidth") ?? "1");
    if (totalWidth > 0 && myWidth > 0) {
      setExtendViewport({ offsetX, totalWidth, myWidth });
    }
  }, [displayMode, searchParams]);

  // ===== 视频同步逻辑（extend + video）=====
  const isMaster = displayMode === "extend" && extendViewport?.offsetX === 0;
  const isSlave = displayMode === "extend" && extendViewport != null && extendViewport.offsetX > 0;

  // Master：每 5s 广播 currentTime
  useEffect(() => {
    if (!isMaster || wallpaper?.type !== "video") return;

    const interval = setInterval(() => {
      const video = videoRef.current;
      if (video && !video.paused) {
        emit(EVENTS.VIDEO_SYNC, { current_time: video.currentTime });
      }
    }, 5000);

    return () => clearInterval(interval);
  }, [isMaster, wallpaper?.type]);

  // Slave：监听同步事件，漂移 > 0.1s 时校准
  useEffect(() => {
    if (!isSlave || wallpaper?.type !== "video") return;

    const unlisten = listen(EVENTS.VIDEO_SYNC, (payload) => {
      const video = videoRef.current;
      if (!video) return;

      const drift = Math.abs(video.currentTime - payload.current_time);
      if (drift > 0.1) {
        video.currentTime = payload.current_time;
      }
    });

    return () => { unlisten.then((fn) => fn()); };
  }, [isSlave, wallpaper?.type]);

  // 无数据时黑屏
  if (!wallpaper) {
    return (
      <div
        className="h-screen w-screen bg-black"
        style={{ pointerEvents: "none", userSelect: "none", ...ncCompensationStyle }}
      />
    );
  }

  const src = convertFileSrc(wallpaper.file_path);

  // extend 模式：裁剪渲染
  if (displayMode === "extend" && extendViewport) {
    const { offsetX, totalWidth, myWidth } = extendViewport;
    const scale = totalWidth / myWidth;
    const translateX = -(offsetX / myWidth) * 100;

    const extendStyle: React.CSSProperties = {
      width: `${scale * 100}%`,
      height: "100%",
      objectFit: "cover" as const,
      transform: `translateX(${translateX}%)`,
    };

    return (
      <div
        className="h-screen w-screen overflow-hidden bg-black"
        style={{ pointerEvents: "none", userSelect: "none", ...ncCompensationStyle }}
      >
        {wallpaper.type === "video" ? (
          <video
            ref={videoRef}
            src={src}
            autoPlay
            loop
            muted
            playsInline
            style={extendStyle}
          />
        ) : (
          <img
            src={src}
            alt=""
            draggable={false}
            style={extendStyle}
          />
        )}
      </div>
    );
  }

  // independent / mirror 模式
  return (
    <div
      className="h-screen w-screen overflow-hidden bg-black"
      style={{ pointerEvents: "none", userSelect: "none", ...ncCompensationStyle }}
    >
      {wallpaper.type === "video" ? (
        <video
          ref={videoRef}
          src={src}
          autoPlay
          loop
          muted
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