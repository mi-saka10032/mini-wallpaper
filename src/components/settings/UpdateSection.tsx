import { memo, useCallback, useEffect, useMemo, type FC } from "react";
import {
  ArrowDownToLine,
  CheckCircle2,
  Loader2,
  RefreshCw,
  Sparkles,
  TriangleAlert,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { useUpdaterStore } from "@/stores/updaterStore";

/** 将字节数格式化为易读文本 */
function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

/**
 * 关于与更新区块
 *
 * 对应需求 2：在设置栏提供手动检测更新入口，点击后比对版本号，
 * 有新版本时提供「下载更新」。与启动静默检查共用 updaterStore，
 * 因此启动时已发现的新版本在此处会直接呈现为可下载态。
 */
const UpdateSection: FC = memo(() => {
  const { t } = useTranslation();

  const phase = useUpdaterStore((s) => s.phase);
  const currentVersion = useUpdaterStore((s) => s.currentVersion);
  const update = useUpdaterStore((s) => s.update);
  const progress = useUpdaterStore((s) => s.progress);
  const error = useUpdaterStore((s) => s.error);
  const initVersion = useUpdaterStore((s) => s.initVersion);
  const runCheck = useUpdaterStore((s) => s.runCheck);
  const startInstall = useUpdaterStore((s) => s.startInstall);

  // 兜底：若启动检查因异常未写入版本号，进入本面板时补读一次
  useEffect(() => {
    if (!currentVersion) initVersion();
  }, [currentVersion, initVersion]);

  // 手动检查：非静默模式，失败会在面板内显示错误
  const handleCheck = useCallback(() => {
    runCheck(false);
  }, [runCheck]);

  const handleDownload = useCallback(() => {
    startInstall();
  }, [startInstall]);

  const isChecking = phase === "checking";
  const isBusy = phase === "downloading" || phase === "installing";
  const hasUpdate = phase === "available" && update !== null;

  /** 下载进度百分比；总长未知时返回 null（展示不确定态） */
  const percent = useMemo(() => {
    if (!progress || progress.total === null || progress.total <= 0) return null;
    return Math.min(100, Math.round((progress.downloaded / progress.total) * 100));
  }, [progress]);

  return (
    <section className="space-y-5">
      <div>
        <h3 className="text-[15px] font-semibold text-foreground">{t("update.navUpdate")}</h3>
        <p className="mt-1 text-[11px] leading-relaxed text-foreground/45">
          {t("update.sectionDesc")}
        </p>
      </div>

      {/* 版本信息 + 检查入口 */}
      <div className="rounded-lg border border-border/50 bg-card">
        <div className="flex items-center justify-between gap-3 px-4 py-3.5">
          <div className="min-w-0 space-y-0.5">
            <span className="text-[13px] font-medium text-foreground">
              {t("update.currentVersion")}
            </span>
            <p className="text-[11px] text-foreground/45 tabular-nums">
              {currentVersion ? `v${currentVersion}` : t("update.versionUnknown")}
            </p>
          </div>
          <button
            type="button"
            onClick={handleCheck}
            disabled={isChecking || isBusy}
            className="flex shrink-0 items-center gap-1.5 rounded-md border border-border/60 px-3 py-1.5 text-[12px] font-medium text-foreground/80 transition-colors hover:bg-primary-hover hover:text-foreground disabled:cursor-not-allowed disabled:opacity-55"
          >
            {isChecking ? (
              <Loader2 className="size-3.5 animate-spin" />
            ) : (
              <RefreshCw className="size-3.5" />
            )}
            {isChecking ? t("update.checking") : t("update.checkNow")}
          </button>
        </div>

        {/* 已是最新 */}
        {phase === "latest" && (
          <>
            <div className="mx-4 h-px bg-border/30" />
            <div className="flex items-center gap-2 px-4 py-3">
              <CheckCircle2 className="size-4 shrink-0 text-emerald-500" />
              <span className="text-[12px] text-foreground/70">{t("update.isLatest")}</span>
            </div>
          </>
        )}

        {/* 发现新版本 */}
        {hasUpdate && (
          <>
            <div className="mx-4 h-px bg-border/30" />
            <div className="space-y-2.5 px-4 py-3.5">
              <div className="flex items-start gap-2">
                <Sparkles className="mt-0.5 size-4 shrink-0 text-primary" />
                <div className="min-w-0 flex-1">
                  <p className="text-[13px] font-medium text-foreground">
                    {t("update.foundNew", { version: update.version })}
                  </p>
                  {update.date && (
                    <p className="mt-0.5 text-[11px] text-foreground/45">
                      {t("update.publishedAt", { date: update.date })}
                    </p>
                  )}
                </div>
              </div>

              {/* 更新说明：内容可能较长，限高滚动，避免撑破面板 */}
              {update.notes && (
                <div className="max-h-28 overflow-y-auto rounded-md bg-surface/70 px-3 py-2">
                  <p className="whitespace-pre-wrap text-[11px] leading-relaxed text-foreground/60">
                    {update.notes}
                  </p>
                </div>
              )}

              <button
                type="button"
                onClick={handleDownload}
                className="flex w-full items-center justify-center gap-1.5 rounded-md bg-primary px-3 py-2 text-[12px] font-medium text-primary-foreground transition-opacity hover:opacity-90"
              >
                <ArrowDownToLine className="size-3.5" />
                {t("update.downloadNow")}
              </button>
            </div>
          </>
        )}

        {/* 下载 / 安装进行中 */}
        {isBusy && (
          <>
            <div className="mx-4 h-px bg-border/30" />
            <div className="space-y-2 px-4 py-3.5">
              <div className="flex items-center justify-between">
                <span className="flex items-center gap-1.5 text-[12px] text-foreground/70">
                  <Loader2 className="size-3.5 animate-spin text-primary" />
                  {phase === "installing" ? t("update.installing") : t("update.downloading")}
                </span>
                {percent !== null && (
                  <span className="text-[11px] tabular-nums text-foreground/45">{percent}%</span>
                )}
              </div>

              {/* 进度条：总长未知时以脉冲动画表达不确定进度 */}
              <div className="h-1.5 w-full overflow-hidden rounded-full bg-surface">
                <div
                  className={
                    percent === null
                      ? "h-full w-1/3 animate-pulse rounded-full bg-primary"
                      : "h-full rounded-full bg-primary transition-all duration-200"
                  }
                  style={percent === null ? undefined : { width: `${percent}%` }}
                />
              </div>

              {progress && (
                <p className="text-[11px] tabular-nums text-foreground/40">
                  {progress.total !== null
                    ? `${formatBytes(progress.downloaded)} / ${formatBytes(progress.total)}`
                    : formatBytes(progress.downloaded)}
                </p>
              )}

              <p className="text-[11px] leading-relaxed text-foreground/45">
                {t("update.installHint")}
              </p>
            </div>
          </>
        )}

        {/* 失败态：仅手动检查会走到这里 */}
        {phase === "error" && (
          <>
            <div className="mx-4 h-px bg-border/30" />
            <div className="flex items-start gap-2 px-4 py-3">
              <TriangleAlert className="mt-0.5 size-4 shrink-0 text-destructive" />
              <div className="min-w-0">
                <p className="text-[12px] text-foreground/70">{t("update.checkFailed")}</p>
                {error && (
                  <p className="mt-0.5 break-words text-[11px] leading-relaxed text-foreground/40">
                    {error}
                  </p>
                )}
              </div>
            </div>
          </>
        )}
      </div>

      <p className="px-1 text-[11px] leading-relaxed text-foreground/45">
        {t("update.autoCheckHint")}
      </p>
    </section>
  );
});

UpdateSection.displayName = "UpdateSection";

export default UpdateSection;
