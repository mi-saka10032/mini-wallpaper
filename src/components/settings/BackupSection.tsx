import React from "react";
import { useTranslation } from "react-i18next";
import { Download, Upload } from "lucide-react";
import { Button } from "@/components/ui/button";

interface BackupSectionProps {
  backupBusy: boolean;
  backupMsg: string | null;
  dataSize: number | null;
  progress: { current: number; total: number } | null;
  formatSize: (bytes: number) => string;
  onExport: () => void;
  onImport: () => void;
}

/** 备份/导出设置区块 - Win11 Fluent 风格 */
const BackupSection: React.FC<BackupSectionProps> = React.memo(({
  backupBusy,
  backupMsg,
  dataSize,
  progress,
  formatSize,
  onExport,
  onImport,
}) => {
  const { t } = useTranslation();

  return (
    <section className="space-y-5">
      <div>
        <h3 className="text-[15px] font-semibold text-foreground">
          {t("settings.navBackup")}
        </h3>
        <p className="mt-1 text-[11px] leading-relaxed text-foreground/45">
          {t("settings.backupDesc")}
        </p>
      </div>

      {/* 数据概览卡片 */}
      {dataSize !== null && (
        <div className="rounded-lg border border-border/50 bg-card">
          <div className="flex items-center justify-between px-4 py-3">
            <span className="text-[13px] text-foreground/60">{t("settings.dataSize")}</span>
            <span className="text-[13px] font-medium tabular-nums">{formatSize(dataSize)}</span>
          </div>
        </div>
      )}

      {/* 操作卡片 */}
      <div className="rounded-lg border border-border/50 bg-card">
        <div className="flex items-center gap-3 px-4 py-3.5">
          <Button
            variant="outline"
            size="sm"
            disabled={backupBusy}
            onClick={onExport}
            className="h-8 gap-1.5 rounded-md border-border/60 text-[12px] hover:bg-foreground/4"
          >
            <Upload className="size-3.5" />
            {t("settings.export")}
          </Button>

          <Button
            variant="outline"
            size="sm"
            disabled={backupBusy}
            onClick={onImport}
            className="h-8 gap-1.5 rounded-md border-border/60 text-[12px] hover:bg-foreground/4"
          >
            <Download className="size-3.5" />
            {t("settings.import")}
          </Button>
        </div>

        {/* 进度条 */}
        {backupBusy && progress && progress.total > 0 && (
          <>
            <div className="mx-4 h-px bg-border/30" />
            <div className="px-4 py-3 space-y-2">
              <div className="flex items-center justify-between text-[11px] text-foreground/45">
                <span>{progress.current} / {progress.total}</span>
                <span className="tabular-nums">{Math.round((progress.current / progress.total) * 100)}%</span>
              </div>
              <div className="h-1.5 w-full overflow-hidden rounded-full bg-foreground/5">
                <div
                  className="h-full rounded-full bg-primary transition-all duration-300 ease-out"
                  style={{ width: `${(progress.current / progress.total) * 100}%` }}
                />
              </div>
            </div>
          </>
        )}
      </div>

      {/* 状态消息 */}
      {backupMsg && (
        <p className="text-[11px] text-foreground/50 px-1">{backupMsg}</p>
      )}
    </section>
  );
});

BackupSection.displayName = "BackupSection";

export default BackupSection;
