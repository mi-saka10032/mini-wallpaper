import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { save, open as openDialog } from "@tauri-apps/plugin-dialog";
import { listen, EVENTS } from "@/api/event";
import { invoke } from "@/api/invoke";
import { COMMANDS } from "@/api/config";

export function useBackup(activeSection: string) {
  const { t } = useTranslation();
  const [backupBusy, setBackupBusy] = useState(false);
  const [backupMsg, setBackupMsg] = useState<string | null>(null);
  const [dataSize, setDataSize] = useState<number | null>(null);
  const [progress, setProgress] = useState<{ current: number; total: number } | null>(null);

  // 监听 backup-progress 事件
  useEffect(() => {
    const unlisten = listen(EVENTS.BACKUP_PROGRESS, (payload) => {
      setProgress(payload);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // 进入 backup 分组时获取数据大小
  useEffect(() => {
    if (activeSection === "backup") {
      invoke(COMMANDS.GET_DATA_SIZE).then(setDataSize).catch(() => {});
    }
  }, [activeSection]);

  const formatSize = useCallback((bytes: number) => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
  }, []);

  const handleExport = useCallback(async () => {
    const outputPath = await save({
      defaultPath: `mini-wallpaper-backup-${new Date().toISOString().slice(0, 10)}.zip`,
      filters: [{ name: "ZIP", extensions: ["zip"] }],
    });
    if (!outputPath) return;
    setBackupBusy(true);
    setBackupMsg(null);
    setProgress(null);
    try {
      await invoke(COMMANDS.EXPORT_BACKUP, { outputPath });
      setBackupMsg(t("settings.exportSuccess"));
    } catch (e) {
      setBackupMsg(t("settings.exportFailed") + ": " + String(e));
    } finally {
      setBackupBusy(false);
      setProgress(null);
    }
  }, [t]);

  const handleImport = useCallback(async () => {
    const selected = await openDialog({
      multiple: false,
      filters: [{ name: "ZIP", extensions: ["zip"] }],
    });
    if (!selected) return;
    setBackupBusy(true);
    setBackupMsg(null);
    setProgress(null);
    try {
      const count = await invoke(COMMANDS.IMPORT_BACKUP, { zipPath: selected });
      setBackupMsg(t("settings.importSuccess", { count }));
    } catch (e) {
      setBackupMsg(t("settings.importFailed") + ": " + String(e));
    } finally {
      setBackupBusy(false);
      setProgress(null);
    }
  }, [t]);

  return {
    backupBusy,
    backupMsg,
    dataSize,
    progress,
    formatSize,
    handleExport,
    handleImport,
  };
}
