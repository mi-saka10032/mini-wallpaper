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
    // display_mode 不再从 config 读取，由全局 app_setting 控制

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
   * 每个窗口根据自己的 monitorId 确定在虚拟画布中的裁剪区域
   */
  const computeExtendViewport = useCallback(async () => {
    if (!monitorId) return;

    try {
      const monitors = await availableMonitors();
      if (monitors.length === 0) return;

      // 获取当前窗口的 scaleFactor，用于将物理像素转换为 CSS 逻辑像素
      // availableMonitors() 返回的 size/position 均为物理像素，
      // 而 CSS 中 width/left 等属性使用逻辑像素，两者在高 DPI 下不一致
      const scaleFactor = await getCurrentWindow().scaleFactor();

      // 计算虚拟画布的 bounding box（所有显示器组成的最小矩形，物理像素）
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

      // 物理像素 → CSS 逻辑像素（除以 scaleFactor）
      const totalWidth = (maxX - minX) / scaleFactor;
      const totalHeight = (maxY - minY) / scaleFactor;

      // 找到当前窗口所属的显示器
      const currentMonitor = monitors.find(
        (m) => (m.name ?? `monitor_${monitors.indexOf(m)}`) === monitorId
      );

      if (!currentMonitor) {
        console.warn("[WallpaperRenderer] Monitor not found for extend:", monitorId);
        return;
      }

      // 当前显示器相对于虚拟画布左上角的偏移量（物理像素 → 逻辑像素）
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

  // 初始化
  useEffect(() => {
    if (!monitorId) return;

    // 从 app_setting 读取全局 display_mode
    invoke(COMMANDS.GET_SETTING, { key: "display_mode" }, { silent: true }).then((val) => {
      if (val) setDisplayMode(val);
    }).catch(() => {});

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

  // 监听壁纸清空事件（壁纸被删除且无后续壁纸可切换时，清空显示）
  useEffect(() => {
    if (!monitorId) return;
    const unlisten = listen(EVENTS.WALLPAPER_CLEARED, (payload) => {
      if (payload.monitor_id === monitorId) {
        setWallpaper(null);
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [monitorId]);

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

        // extend 模式下，通过 availableMonitors() 自行计算视口
        if (payload.display_mode === "extend") {
          computeExtendViewport();
        } else {
          setExtendViewport(null);
        }
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [monitorId, computeExtendViewport]);

  // extend 模式初始化：首次加载时如果已是 extend 模式，计算视口
  useEffect(() => {
    if (displayMode === "extend") {
      computeExtendViewport();
    } else {
      setExtendViewport(null);
    }
  }, [displayMode, computeExtendViewport]);

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
  // 原理：将图片/视频拉伸到整个虚拟画布大小（所有显示器组成的 bounding box），
  // 然后通过 position:absolute + 负偏移，只显示当前显示器对应的区域。
  // 容器 overflow:hidden 自动裁剪掉超出部分。
  if (displayMode === "extend" && extendViewport) {
    const { offsetX, offsetY, totalWidth, totalHeight, myWidth, myHeight } = extendViewport;

    // 图片/视频的实际渲染尺寸 = 虚拟画布大小（逻辑像素）
    // 偏移量 = 当前显示器在画布中的位置（取负值，向左上方移动）
    // objectFit: "fill" 强制拉伸到指定宽高，不保持宽高比
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