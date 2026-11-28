import { useCallback, useEffect, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FolderPlus, RefreshCw, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import {
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { toast } from "@/components/ui/toast";
import { cn } from "@/lib/utils";
import {
  addWatchedFolder,
  listWatchedFolders,
  removeWatchedFolder,
  syncWatchedFolder,
  WATCHED_PATH_INVALID,
} from "@/api/watchedFolder";
import type { AddWatchedFolderResult, WatchedFolderView } from "@/api/config";

interface WatchedFoldersDialogProps {
  /** 新增成功后关闭本弹窗，由外层打开独立的名称确认弹窗 */
  onAdded: (result: AddWatchedFolderResult) => void;
  onClose: () => void;
}

const WatchedFoldersDialog: React.FC<WatchedFoldersDialogProps> = ({ onAdded, onClose }) => {
  const { t } = useTranslation();
  const [folders, setFolders] = useState<WatchedFolderView[]>([]);
  const [busy, setBusy] = useState(false);
  const noticeItems = t("watched.noticeItems", { returnObjects: true }) as string[];

  const refresh = useCallback(async () => {
    try {
      setFolders(await listWatchedFolders());
    } catch (e) {
      console.error("[Watched] list failed", e);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const handleAdd = useCallback(async () => {
    const selected = await openDialog({ directory: true, multiple: false });
    if (typeof selected !== "string") return;
    setBusy(true);
    try {
      const result = await addWatchedFolder(selected);
      // 先关弹窗，再让外层弹独立的名称确认弹窗：
      // 两个 Dialog 不在同一层级，避免遮罩叠加导致点击穿透。
      onClose();
      onAdded(result);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  }, [onAdded, onClose]);

  const handleRescan = useCallback(
    async (folder: WatchedFolderView) => {
      if (folder.collection_id === null) return;
      setBusy(true);
      try {
        const report = await syncWatchedFolder(folder.collection_id);
        toast.success(t("watched.rescanDone", { count: report.added }));
        await refresh();
      } catch (e) {
        const message = String(e);
        toast.error(message.includes(WATCHED_PATH_INVALID) ? t("watched.pathInvalid") : message);
        await refresh();
      } finally {
        setBusy(false);
      }
    },
    [refresh, t],
  );

  const handleRemove = useCallback(
    async (folder: WatchedFolderView) => {
      setBusy(true);
      try {
        await removeWatchedFolder(folder.id);
        await refresh();
      } catch (e) {
        toast.error(String(e));
      } finally {
        setBusy(false);
      }
    },
    [refresh],
  );

  return (
    <DialogContent className="flex max-h-[85vh] flex-col sm:max-w-lg">
      <DialogHeader>
        <DialogTitle>{t("watched.title")}</DialogTitle>
        <DialogDescription asChild>
          <ul className="list-disc space-y-1 pl-4 text-left text-sm text-muted-foreground">
            {noticeItems.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
        </DialogDescription>
      </DialogHeader>

      {/* 固定最大高度并开启滚动，目录再多也不会把弹窗撑出屏幕 */}
      <div className="min-h-[80px] max-h-[240px] flex-1 space-y-2 overflow-y-auto pr-1">
        {folders.length === 0 && (
          <p className="py-6 text-center text-sm text-muted-foreground">{t("watched.empty")}</p>
        )}
        {folders.map((folder) => (
          <div key={folder.id} className="rounded-md border border-border/60 p-2.5 text-sm">
            <div className="flex items-center gap-2">
              <span
                className={cn(
                  "shrink-0 rounded px-1.5 py-0.5 text-xs",
                  folder.valid === 1
                    ? "bg-primary/10 text-primary"
                    : "bg-destructive/10 text-destructive",
                )}
              >
                {folder.valid === 1 ? t("watched.valid") : t("watched.invalid")}
              </span>
              <span className="min-w-0 flex-1 truncate" title={folder.path}>
                {folder.path}
              </span>
              {folder.valid === 1 && (
                <Button
                  variant="ghost"
                  size="icon-xs"
                  disabled={busy}
                  title={t("watched.rescan")}
                  onClick={() => void handleRescan(folder)}
                >
                  <RefreshCw className="size-3.5" />
                </Button>
              )}
              <Button
                variant="ghost"
                size="icon-xs"
                disabled={busy}
                title={t("watched.remove")}
                onClick={() => void handleRemove(folder)}
              >
                <Trash2 className="size-3.5" />
              </Button>
            </div>
            {folder.valid === 0 && (
              <p className="mt-1.5 text-xs text-destructive/80">{t("watched.invalidHint")}</p>
            )}
          </div>
        ))}
      </div>

      <Separator />

      {/* 唯一功能入口：底部居中 */}
      {/* DialogFooter 默认有 sm:justify-end，必须用同断点类覆盖才真正居中 */}
      <DialogFooter className="justify-center sm:justify-center">
        <Button disabled={busy} onClick={() => void handleAdd()}>
          <FolderPlus className="size-4" />
          {t("watched.add")}
        </Button>
      </DialogFooter>
    </DialogContent>
  );
};

export default WatchedFoldersDialog;
