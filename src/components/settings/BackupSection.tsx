import { memo, type FC } from "react";
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
const BackupSection: FC<BackupSectionProps> = memo(
  ({ backupBusy, backupMsg, dataSize, progress, formatSize, onExport, onImport }) => {
    const { t } = useTranslation();

    return (
      <section className="space-y-5">
        <div>
          <h3 className="text-base font-semibold text-foreground">{t("settings.navBackup")}</h3>
          <p className="mt-1 text-xs leading-relaxed text-foreground/40">
            {t("settings.backupDesc")}
          </p>
        </div>

        {/* 数据概览卡片 */}
        {dataSize !== null && (
          <div className="rounded-lg border border-border/50 bg-card">
            <div className="flex items-center justify-between px-4 py-3">
              <span className="text-sm text-muted-foreground">{t("settings.dataSize")}</span>
              <span className="text-sm font-medium tabular-nums">{formatSize(dataSize)}</span>
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
              className="h-8 gap-1.5 rounded-md border-border/50 text-xs hover:bg-primary-hover"
            >
              <Upload className="size-3.5" />
              {t("settings.export")}
            </Button>

            <Button
              variant="outline"
              size="sm"
              disabled={backupBusy}
              onClick={onImport}
              className="h-8 gap-1.5 rounded-md border-border/50 text-xs hover:bg-primary-hover"
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
                <div className="flex items-center justify-between text-xs text-foreground/40">
                  <span>
                    {progress.current} / {progress.total}
                  </span>
                  <span className="tabular-nums">
                    {Math.round((progress.current / progress.total) * 100)}%
                  </span>
                </div>
                <div className="h-1.5 w-full overflow-hidden rounded-full bg-foreground/5">
                  <div
                    className="h-full rounded-full bg-primary transition-[width] duration-300 ease-out"
                    style={{ width: `${(progress.current / progress.total) * 100}%` }}
                  />
                </div>
              </div>
            </>
          )}
        </div>

        {/* 状态消息 */}
        {backupMsg && <p className="text-xs text-muted-foreground px-1">{backupMsg}</p>}
      </section>
    );
  },
);

BackupSection.displayName = "BackupSection";

export default BackupSection;
