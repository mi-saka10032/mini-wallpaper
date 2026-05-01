import { useCallback, useEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { convertFileSrc } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { availableMonitors, getCurrentWindow } from "@tauri-apps/api/window";
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
    offsetY: number;
    totalWidth: number;
    totalHeight: number;
    myWidth: number;
    myHeight: number;
  } | null>(null);

  const videoRef = useRef<HTMLVideoElement>(null);
  const [volume, setVolume] = useState<number>(0);

  // ===== Effect 1: 壁纸窗口 body 透明 =====
  useEffect(() => {
    document.body.classList.add("wallpaper-body");
    return () => {
      document.body.classList.remove("wallpaper-body");
    };
  }, []);

  // ===== Effect 2: 禁用所有用户输入事件 =====
  useEffect(() => {
    const blockAll = (e: Event) => {
      e.preventDefault();
      e.stopPropagation();
    };

    const mouseEvents = [
      "contextmenu", "click", "dblclick", "mousedown", "mouseup",
      "mousemove", "mouseover", "mouseout", "mouseenter", "mouseleave",
      "wheel", "auxclick",
    ];
    const keyEvents = ["keydown", "keyup", "keypress"];
    const dragEvents = ["dragover", "dragenter", "dragleave", "drop", "drag", "dragstart", "dragend"];
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

  /**
   * 通过 Tauri availableMonitors() API 计算 extend 模式下的视口参数
   */
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
        console.warn("[WallpaperRenderer] Monitor not found for extend:", monitorId);
        return;
      }

      const offsetX = (currentMonitor.position.x - minX) / scaleFactor;
      const offsetY = (currentMonitor.position.y - minY) / scaleFactor;
      const myWidth = currentMonitor.size.width / scaleFactor;
      const myHeight = currentMonitor.size.height / scaleFactor;

      console.log("[WallpaperRenderer] extend viewport computed:", {
        scaleFactor,
        totalWidth, totalHeight,
        offsetX, offsetY,
        myWidth, myHeight,
        rawPhysical: { totalW: maxX - minX, totalH: maxY - minY },
      });

      setExtendViewport({ offsetX, offsetY, totalWidth, totalHeight, myWidth, myHeight });
    } catch (e) {
      console.error("[WallpaperRenderer] Failed to compute extend viewport:", e);
    }
  }, [monitorId]);

  // ===== Effect 3: 初始化（加载 config + 音量 + displayMode） =====
  useEffect(() => {
    if (!monitorId) return;

    // 读取全局 display_mode
    invoke(COMMANDS.GET_SETTING, { key: "display_mode" }, { silent: true }).then((val) => {
      if (val) setDisplayMode(val);
    }).catch(() => {});

    // 读取 monitor config
    invoke(COMMANDS.GET_MONITOR_CONFIG, { monitorId }, { silent: true }).then((config) => {
      if (config) loadFromConfig(config);
    });

    // 读取全局音量
    invoke(COMMANDS.GET_SETTING, { key: "global_volume" }, { silent: true }).then((val) => {
      const v = Number(val ?? "0");
      setVolume(Math.min(Math.max(v, 0), 100));
    }).catch(() => {});
  }, [monitorId, loadFromConfig]);

  // ===== Effect 4: 统一事件监听（壁纸变更、清空、fitMode、displayMode、音量） =====
  useEffect(() => {
    if (!monitorId) return;

    const unlisteners: Promise<() => void>[] = [];

    // 壁纸切换事件
    unlisteners.push(
      listen(EVENTS.WALLPAPER_CHANGED, (payload) => {
        if (payload.monitor_id === monitorId) {
          invoke(COMMANDS.GET_MONITOR_CONFIG, { monitorId }, { silent: true }).then((config) => {
            if (config) loadFromConfig(config);
          });
        }
      })
    );

    // 壁纸清空事件
    unlisteners.push(
      listen(EVENTS.WALLPAPER_CLEARED, (payload) => {
        if (payload.monitor_id === monitorId) {
          setWallpaper(null);
        }
      })
    );

    // fitMode 变更事件
    unlisteners.push(
      listen(EVENTS.FIT_MODE_CHANGED, (payload) => {
        if (payload.monitor_id === monitorId) {
          setFitMode(payload.fit_mode);
        }
      })
    );

    // displayMode 变更事件
    unlisteners.push(
      listen(EVENTS.DISPLAY_MODE_CHANGED, (payload) => {
        if (payload.monitor_id === monitorId) {
          setDisplayMode(payload.display_mode);
        }
      })
    );

    // 音量变更事件
    unlisteners.push(
      listen(EVENTS.VOLUME_CHANGED, (payload) => {
        setVolume(Math.min(Math.max(payload.volume, 0), 100));
      })
    );

    return () => {
      for (const p of unlisteners) {
        p.then((fn) => fn());
      }
    };
  }, [monitorId, loadFromConfig]);

  // ===== Effect 5: displayMode 变化时计算/清除 extend viewport =====
  useEffect(() => {
    if (displayMode === "extend") {
      computeExtendViewport();
    } else {
      setExtendViewport(null);
    }
  }, [displayMode, computeExtendViewport]);

  // ===== Effect 6: 音量同步到 video 元素 =====
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    video.volume = volume / 100;
    video.muted = volume === 0;
  }, [volume, wallpaper]);

  // ===== 视频同步逻辑（extend + video）=====
  const isMaster = displayMode === "extend" && extendViewport?.offsetX === 0;
  const isSlave = displayMode === "extend" && extendViewport != null && extendViewport.offsetX > 0;

  // ===== Effect 7a: Master 每 5s 广播 currentTime =====
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

  // ===== Effect 7b: Slave 监听同步事件 =====
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
            style={{
              display: 'block',
              width: '100%',
              height: '100%',
              objectFit: 'fill',
            }}
          />
        ) : (
          <img
            src={src}
            alt=""
            draggable={false}
            style={{
              display: 'block',
              width: '100%',
              height: '100%',
              objectFit: 'fill',
            }}
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