import { useEffect, useState, useCallback } from "react";
import { useSearchParams } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  SkipForward,
  SkipBack,
  Pause,
  Play,
  AppWindow,
  X,
} from "lucide-react";

/** 动作类型 → 图标 & 颜色映射 */
const ACTION_CONFIG: Record<string, { icon: React.ElementType; color: string; bg: string }> = {
  next: { icon: SkipForward, color: "text-blue-400", bg: "bg-blue-500/10" },
  prev: { icon: SkipBack, color: "text-blue-400", bg: "bg-blue-500/10" },
  togglePause_paused: { icon: Pause, color: "text-amber-400", bg: "bg-amber-500/10" },
  togglePause_resumed: { icon: Play, color: "text-emerald-400", bg: "bg-emerald-500/10" },
  openMain: { icon: AppWindow, color: "text-purple-400", bg: "bg-purple-500/10" },
};

/**
 * ActionToast - 独立窗口 Toast 通知组件
 *
 * 由 Rust 端 toast_manager 创建独立 WebviewWindow，
 * URL: /toast?action=xxx&message=xxx&label=xxx
 *
 * 仿 Windows 11 右下角通知样式，支持：
 * - 点击关闭按钮手动关闭
 * - duration 超时后自动关闭（由 Rust 端 spawn 控制）
 * - 入场/退场动画
 */
const ActionToast: React.FC = () => {
  const [searchParams] = useSearchParams();
  const action = searchParams.get("action") || "next";
  const message = searchParams.get("message") || "";
  const label = searchParams.get("label") || "";
  /** 自动关闭时长（毫秒），由 Rust 端透传，缺省 3000ms */
  const duration = (() => {
    const raw = Number(searchParams.get("duration"));
    return Number.isFinite(raw) && raw > 0 ? raw : 3000;
  })();

  const [visible, setVisible] = useState(false);
  const [exiting, setExiting] = useState(false);

  // Toast 窗口专属 body class：保证根容器完全透明，无灰色底
  useEffect(() => {
    document.body.classList.add("toast-body");
    return () => {
      document.body.classList.remove("toast-body");
    };
  }, []);

  // 入场动画
  useEffect(() => {
    const timer = setTimeout(() => setVisible(true), 50);
    return () => clearTimeout(timer);
  }, []);

  // 关闭 toast 窗口
  const handleClose = useCallback(async () => {
    setExiting(true);
    // 等待退场动画完成后通知后端销毁窗口
    setTimeout(async () => {
      try {
        await invoke("close_toast_window", { label });
      } catch {
        // 如果 command 失败，直接关闭当前窗口
        await getCurrentWindow().close();
      }
    }, 200);
  }, [label]);

  // duration 到点后自动关闭（含退场动画），用户手动关闭时定时器随卸载清理
  useEffect(() => {
    const timer = setTimeout(() => {
      void handleClose();
    }, duration);
    return () => clearTimeout(timer);
  }, [duration, handleClose]);

  // 解析动作配置
  const configKey = action === "togglePause"
    ? (message.includes("暂停") || message.includes("Paused") ? "togglePause_paused" : "togglePause_resumed")
    : action;
  const config = ACTION_CONFIG[configKey] || ACTION_CONFIG.next;
  const Icon = config.icon;

  return (
    <div
      className="h-screen w-screen overflow-hidden bg-transparent"
      style={{ pointerEvents: "auto" }}
    >
      <div
        className={`
          flex items-center gap-3 mx-2 my-2 px-4 py-3
          rounded-xl border border-white/10
          bg-[#2d2d2d]/95 backdrop-blur-xl
          shadow-[0_8px_32px_rgba(0,0,0,0.4),0_2px_8px_rgba(0,0,0,0.3)]
          transition-all duration-300 ease-out
          ${visible && !exiting
            ? "opacity-100 translate-x-0"
            : "opacity-0 translate-x-8"
          }
        `}
      >
        {/* 动作图标 */}
        <div className={`flex-shrink-0 rounded-lg p-2 ${config.bg}`}>
          <Icon className={`size-4 ${config.color}`} />
        </div>

        {/* 消息内容 */}
        <div className="flex-1 min-w-0">
          <p className="text-[12px] font-medium text-white/90 truncate">
            {message}
          </p>
          <p className="text-[10px] text-white/40 mt-0.5">
            Mini Wallpaper
          </p>
        </div>

        {/* 关闭按钮 */}
        <button
          type="button"
          onClick={handleClose}
          className="flex-shrink-0 rounded-md p-1 text-white/30 hover:text-white/70 hover:bg-white/10 transition-colors"
        >
          <X className="size-3.5" />
        </button>
      </div>
    </div>
  );
};

export default ActionToast;
