import { memo, useCallback, useEffect, useRef, type FC } from "react";
import { ArrowDownToLine, Sparkles, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useUpdaterStore } from "@/stores/updaterStore";

/** 浮窗自动关闭延迟（毫秒），对应需求「默认 5s 后自动关闭」 */
const AUTO_DISMISS_DELAY = 5000;

/**
 * 新版本提示浮窗
 *
 * 挂载于 Toolbar 右侧设置图标下方（由父级 relative 容器定位）。
 * 行为约定（对应需求 1）：
 * - 仅在启动静默检查发现新版本时出现
 * - 提供「下载更新」入口
 * - 可手动关闭，且默认 5s 后自动关闭
 * - 一旦用户点击下载进入下载/安装阶段，停止自动关闭，避免进度提示被吞掉
 */
const UpdateToast: FC = memo(() => {
  const { t } = useTranslation();

  const phase = useUpdaterStore((s) => s.phase);
  const update = useUpdaterStore((s) => s.update);
  const toastVisible = useUpdaterStore((s) => s.toastVisible);
  const dismissToast = useUpdaterStore((s) => s.dismissToast);
  const startInstall = useUpdaterStore((s) => s.startInstall);

  const timerRef = useRef<number | null>(null);

  const clearTimer = useCallback(() => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  // 仅在「等待用户决定」阶段启动自动关闭计时；进入下载/安装后保持常驻
  useEffect(() => {
    clearTimer();
    if (!toastVisible || phase !== "available") return;

    timerRef.current = window.setTimeout(() => {
      dismissToast();
    }, AUTO_DISMISS_DELAY);

    return clearTimer;
  }, [toastVisible, phase, dismissToast, clearTimer]);

  // 鼠标悬停时暂停自动关闭，移出后重新计时，避免用户正在阅读时浮窗消失
  const handleMouseEnter = useCallback(() => {
    clearTimer();
  }, [clearTimer]);

  const handleMouseLeave = useCallback(() => {
    if (phase !== "available") return;
    clearTimer();
    timerRef.current = window.setTimeout(() => {
      dismissToast();
    }, AUTO_DISMISS_DELAY);
  }, [phase, dismissToast, clearTimer]);

  const handleDownload = useCallback(() => {
    clearTimer();
    startInstall();
  }, [startInstall, clearTimer]);

  if (!toastVisible || !update) return null;

  const isBusy = phase === "downloading" || phase === "installing";

  return (
    <div
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
      className="absolute right-2 top-11 z-50 w-64 animate-in fade-in slide-in-from-top-2 duration-200 rounded-lg border border-border/50 bg-popover/95 p-3 backdrop-blur-sm fluent-shadow-lg"
    >
      <div className="flex items-start gap-2.5">
        <Sparkles className="mt-0.5 size-4 shrink-0 text-primary" />
        <div className="min-w-0 flex-1">
          <p className="text-[13px] font-medium text-foreground">
            {t("update.toastTitle", { version: update.version })}
          </p>
          <p className="mt-0.5 text-[11px] leading-relaxed text-foreground/50">
            {isBusy ? t("update.downloadingHint") : t("update.toastDesc")}
          </p>
        </div>
        <button
          type="button"
          onClick={dismissToast}
          title={t("update.dismiss")}
          className="shrink-0 rounded-sm p-0.5 text-foreground/40 transition-colors hover:bg-primary-hover hover:text-foreground"
        >
          <X className="size-3.5" />
        </button>
      </div>

      <button
        type="button"
        onClick={handleDownload}
        disabled={isBusy}
        className="mt-2.5 flex w-full items-center justify-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-[12px] font-medium text-primary-foreground transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-60"
      >
        <ArrowDownToLine className="size-3.5" />
        {isBusy ? t("update.downloading") : t("update.downloadNow")}
      </button>
    </div>
  );
});

UpdateToast.displayName = "UpdateToast";

export default UpdateToast;
