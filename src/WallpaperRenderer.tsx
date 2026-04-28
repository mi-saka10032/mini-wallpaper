import { useCallback, useEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { convertFileSrc } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
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

  const videoRef = useRef<HTMLVideoElement>(null);

  // ===== 音量状态 =====
  // 初始化时从 DB 读取，后续通过 volume-changed 事件实时更新
  const [volume, setVolume] = useState<number>(0);

  // 初始化读取全局音量设置
  useEffect(() => {
    invoke(COMMANDS.GET_SETTING, { key: "global_volume" }, { silent: true }).then((val) => {
      const v = Number(val ?? "0");
      setVolume(Math.min(Math.max(v, 0), 100));
    }).catch(() => {});
  }, []);

  // 监听音量变更事件（后端 setting effect 广播）
  useEffect(() => {
    const unlisten = listen(EVENTS.VOLUME_CHANGED, (payload) => {
      setVolume(Math.min(Math.max(payload.volume, 0), 100));
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  // 音量变化时同步到 video 元素
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    video.volume = volume / 100;
    video.muted = volume === 0;
  }, [volume, wallpaper]);

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
      const all = await invoke(COMMANDS.GET_WALLPAPERS, { silent: true });
      const found = all.find((w) => w.id === config.wallpaper_id) ?? null;
      setWallpaper(found);
    } catch (e) {
      console.error("[WallpaperRenderer] fetch wallpaper failed:", e);
    }
  }, []);

  // 初始化
  useEffect(() => {
    if (!monitorId) return;
    invoke(COMMANDS.GET_MONITOR_CONFIG, { monitorId }, { silent: true }).then((config) => {
      if (config) loadFromConfig(config);
    });
  }, [monitorId, loadFromConfig]);

  // 监听壁纸切换事件
  useEffect(() => {
    if (!monitorId) return;
    const unlisten = listen(EVENTS.WALLPAPER_CHANGED, (payload) => {
      if (payload.monitor_id === monitorId) {
        invoke(COMMANDS.GET_MONITOR_CONFIG, { monitorId }, { silent: true }).then((config) => {
          if (config) loadFromConfig(config);
        });
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [monitorId, loadFromConfig]);

  // 监听 fitMode 变更事件（直接更新 objectFit 样式，无需重新加载壁纸）
  useEffect(() => {
    if (!monitorId) return;
    const unlisten = listen(EVENTS.FIT_MODE_CHANGED, (payload) => {
      if (payload.monitor_id === monitorId) {
        setFitMode(payload.fit_mode);
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [monitorId]);

  // 监听 displayMode 变更事件（切换渲染模式：independent / mirror / extend）
  useEffect(() => {
    if (!monitorId) return;
    const unlisten = listen(EVENTS.DISPLAY_MODE_CHANGED, (payload) => {
      if (payload.monitor_id === monitorId) {
        setDisplayMode(payload.display_mode);
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [monitorId]);

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
        style={{ pointerEvents: "none", userSelect: "none" }}
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