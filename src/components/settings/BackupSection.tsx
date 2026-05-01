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

/** 备份/导出设置区块 */
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
    <div className="space-y-6">
      <h3 className="text-base font-semibold">{t("settings.navBackup")}</h3>

      {/* 数据大小 */}
      {dataSize !== null && (
        <div className="rounded-md bg-foreground/3 px-4 py-3">
          <div className="flex items-center justify-between">
            <span className="text-sm text-foreground/50">{t("settings.dataSize")}</span>
            <span className="text-sm font-medium">{formatSize(dataSize)}</span>
          </div>
        </div>
      )}

      <p className="text-xs text-foreground/50">
        {t("settings.backupDesc")}
      </p>

      {/* 导出/导入 */}
      <div className="flex items-center gap-3">
        <Button
          variant="outline"
          size="sm"
          disabled={backupBusy}
          onClick={onExport}
          className="gap-1.5"
        >
          <Upload className="size-3.5" />
          {t("settings.export")}
        </Button>

        <Button
          variant="outline"
          size="sm"
          disabled={backupBusy}
          onClick={onImport}
          className="gap-1.5"
        >
          <Download className="size-3.5" />
          {t("settings.import")}
        </Button>
      </div>

      {/* 进度条 */}
      {backupBusy && progress && progress.total > 0 && (
        <div className="space-y-1.5">
          <div className="flex items-center justify-between text-xs text-foreground/50">
            <span>{progress.current} / {progress.total}</span>
            <span>{Math.round((progress.current / progress.total) * 100)}%</span>
          </div>
          <div className="h-2 w-full overflow-hidden rounded-full bg-foreground/5">
            <div
              className="h-full rounded-full bg-primary transition-all duration-200"
              style={{ width: `${(progress.current / progress.total) * 100}%` }}
            />
          </div>
        </div>
      )}

      {/* 状态消息 */}
      {backupMsg && (
        <p className="text-xs text-foreground/50">{backupMsg}</p>
      )}
    </div>
  );
});

BackupSection.displayName = "BackupSection";

export default BackupSection;
