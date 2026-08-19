import { convertFileSrc } from "@tauri-apps/api/core";
import { ArchiveRestore, Film, ImageIcon, Trash2, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import type { Wallpaper } from "@/api/config";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import LazyImage from "@/components/ui/LazyImage";
import { toast } from "@/components/ui/toast";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import VirtualGrid from "@/components/wallpaper/VirtualGrid";
import { cn } from "@/lib/utils";
import { SETTING_KEYS, useSetting } from "@/stores/settingStore";
import { useWallpaperStore } from "@/stores/wallpaperStore";

/** 默认保留天数，与后端 DEFAULT_TRASH_RETENTION_DAYS 保持一致 */
const DEFAULT_RETENTION_DAYS = 30;

/**
 * 计算距离自动清理还剩多少天
 *
 * `deletedAt` 为后端写入的 `YYYY-MM-DD HH:mm:ss` 本地时间字符串。
 * 返回 null 表示无法解析或未启用自动清理，此时不展示剩余天数。
 */
function calcRemainingDays(deletedAt: string | null, retentionDays: number): number | null {
  if (!deletedAt) return null;
  // Safari/WebView 对 "YYYY-MM-DD HH:mm:ss" 解析不稳定，替换为 ISO 风格
  const parsed = new Date(deletedAt.replace(" ", "T"));
  const ts = parsed.getTime();
  if (Number.isNaN(ts)) return null;

  const elapsedDays = (Date.now() - ts) / (1000 * 60 * 60 * 24);
  const remaining = Math.ceil(retentionDays - elapsedDays);
  return remaining > 0 ? remaining : 0;
}

/** 单张回收站壁纸卡片 */
interface TrashCardProps {
  wallpaper: Wallpaper;
  remainingDays: number | null;
  selected: boolean;
  onToggle: (id: number) => void;
  onRestore: (id: number) => void;
  onPurge: (id: number) => void;
}

const TrashCard: React.FC<TrashCardProps> = ({
  wallpaper,
  remainingDays,
  selected,
  onToggle,
  onRestore,
  onPurge,
}) => {
  const { t } = useTranslation();
  const isVideo = wallpaper.type === "video";

  return (
    <div
      className={cn(
        "group relative flex h-full flex-col overflow-hidden rounded-lg border bg-card transition-all duration-150",
        selected
          ? "border-primary ring-2 ring-primary/40"
          : "border-border/50 hover:border-border hover:fluent-shadow",
      )}
    >
      {/* 缩略图区（点击切换选中） */}
      <button
        type="button"
        onClick={() => onToggle(wallpaper.id)}
        className="relative block aspect-video w-full overflow-hidden bg-muted"
      >
        {wallpaper.thumb_path ? (
          <LazyImage
            src={convertFileSrc(wallpaper.thumb_path)}
            alt={wallpaper.name}
            className="size-full object-cover opacity-70 grayscale transition-all duration-200 group-hover:opacity-100 group-hover:grayscale-0"
          />
        ) : (
          <div className="flex size-full items-center justify-center text-foreground/25">
            {isVideo ? <Film className="size-8" strokeWidth={1.25} /> : <ImageIcon className="size-8" strokeWidth={1.25} />}
          </div>
        )}

        {/* 剩余天数角标 */}
        {remainingDays !== null && (
          <span
            className={cn(
              "absolute left-1.5 top-1.5 rounded px-1.5 py-0.5 text-[11px] font-medium backdrop-blur-sm",
              remainingDays <= 3
                ? "bg-destructive/85 text-destructive-foreground"
                : "bg-black/55 text-white",
            )}
          >
            {remainingDays === 0
              ? t("trash.expiringToday")
              : t("trash.daysLeft", { count: remainingDays })}
          </span>
        )}

        {/* 选中标记 */}
        {selected && (
          <span className="absolute right-1.5 top-1.5 flex size-5 items-center justify-center rounded-full bg-primary text-[11px] font-bold text-primary-foreground">
            ✓
          </span>
        )}
      </button>

      {/* 信息 + 操作区 */}
      <div className="flex min-w-0 flex-1 flex-col gap-1.5 p-2">
        <Tooltip>
          <TooltipTrigger asChild>
            <p className="truncate text-xs text-foreground/75">{wallpaper.name}</p>
          </TooltipTrigger>
          <TooltipContent side="top">{wallpaper.name}</TooltipContent>
        </Tooltip>

        <div className="flex items-center gap-1">
          <Button
            variant="outline"
            size="sm"
            className="h-6 flex-1 px-1.5 text-[11px]"
            onClick={() => onRestore(wallpaper.id)}
          >
            <ArchiveRestore className="mr-1 size-3" />
            {t("trash.restore")}
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="size-6 text-destructive hover:bg-destructive/10 hover:text-destructive"
            onClick={() => onPurge(wallpaper.id)}
          >
            <Trash2 className="size-3" />
          </Button>
        </div>
      </div>
    </div>
  );
};

/**
 * 回收站视图
 *
 * 展示已移入回收站的壁纸，支持恢复、彻底删除与清空。
 * 所有破坏性操作（彻底删除 / 清空）均需二次确认。
 *
 * 该视图刻意与主网格解耦：不复用 WallpaperCard，因为回收站内禁止
 * 设为壁纸、加入收藏夹、编辑标签等一切写操作，复用反而会引入大量条件分支。
 */
const TrashView: React.FC = () => {
  const { t } = useTranslation();
  const trashed = useWallpaperStore((s) => s.trashed);
  const trashLoading = useWallpaperStore((s) => s.trashLoading);
  const fetchTrashed = useWallpaperStore((s) => s.fetchTrashed);
  const restoreWallpapers = useWallpaperStore((s) => s.restoreWallpapers);
  const purgeWallpapers = useWallpaperStore((s) => s.purgeWallpapers);
  const emptyTrash = useWallpaperStore((s) => s.emptyTrash);

  const retentionRaw = useSetting(SETTING_KEYS.TRASH_RETENTION_DAYS);
  const autoPurgeRaw = useSetting(SETTING_KEYS.TRASH_AUTO_PURGE);

  // 保留天数 / 是否自动清理（用于剩余天数角标展示）
  const retentionDays = useMemo(() => {
    const parsed = retentionRaw ? Number.parseInt(retentionRaw, 10) : Number.NaN;
    return Number.isFinite(parsed) && parsed > 0 ? parsed : DEFAULT_RETENTION_DAYS;
  }, [retentionRaw]);

  // 缺省（空字符串）视为启用，与后端 purge_expired_on_startup 的缺省行为一致
  const autoPurgeEnabled = useMemo(() => autoPurgeRaw !== "false", [autoPurgeRaw]);

  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set());
  // 待确认的彻底删除目标：number[] = 指定项，"all" = 清空回收站
  const [purgeTarget, setPurgeTarget] = useState<number[] | "all" | null>(null);

  useEffect(() => {
    fetchTrashed();
  }, [fetchTrashed]);

  // 列表变化后剔除已不存在的选中项，避免残留幽灵选中
  useEffect(() => {
    setSelectedIds((prev) => {
      if (prev.size === 0) return prev;
      const alive = new Set(trashed.map((w) => w.id));
      const next = new Set([...prev].filter((id) => alive.has(id)));
      return next.size === prev.size ? prev : next;
    });
  }, [trashed]);

  const toggleSelect = useCallback((id: number) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const selectAll = useCallback(() => {
    setSelectedIds(new Set(trashed.map((w) => w.id)));
  }, [trashed]);

  const clearSelection = useCallback(() => {
    setSelectedIds(new Set());
  }, []);

  const handleRestore = useCallback(
    async (ids: number[]) => {
      if (ids.length === 0) return;
      const count = await restoreWallpapers(ids);
      if (count > 0) {
        toast.success(t("trash.restoredCount", { count }));
      }
      clearSelection();
    },
    [restoreWallpapers, clearSelection, t],
  );

  const handlePurgeConfirm = useCallback(async () => {
    if (purgeTarget === null) return;

    const count =
      purgeTarget === "all" ? await emptyTrash() : await purgeWallpapers(purgeTarget);

    if (count > 0) {
      toast.success(t("trash.purgedCount", { count }));
    }
    setPurgeTarget(null);
    clearSelection();
  }, [purgeTarget, emptyTrash, purgeWallpapers, clearSelection, t]);

  const selectedCount = selectedIds.size;
  const isEmpty = trashed.length === 0;

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {/* 工具栏 */}
      <div className="flex h-10 shrink-0 items-center gap-2 border-b border-border/40 px-4">
        <Trash2 className="size-4 text-foreground/60" />
        <span className="text-[13px] font-medium">{t("trash.title")}</span>
        <span className="text-xs text-foreground/45">
          {t("trash.itemCount", { count: trashed.length })}
        </span>

        <div className="ml-auto flex items-center gap-1.5">
          {selectedCount > 0 ? (
            <>
              <span className="mr-1 text-xs text-foreground/60">
                {t("trash.selectedCount", { count: selectedCount })}
              </span>
              <Button variant="outline" size="sm" className="h-7 text-xs" onClick={clearSelection}>
                {t("trash.clearSelection")}
              </Button>
              <Button
                variant="outline"
                size="sm"
                className="h-7 text-xs"
                onClick={() => handleRestore([...selectedIds])}
              >
                <ArchiveRestore className="mr-1 size-3.5" />
                {t("trash.restoreSelected")}
              </Button>
              <Button
                variant="destructive"
                size="sm"
                className="h-7 text-xs"
                onClick={() => setPurgeTarget([...selectedIds])}
              >
                <Trash2 className="mr-1 size-3.5" />
                {t("trash.purgeSelected")}
              </Button>
            </>
          ) : (
            <>
              {!isEmpty && (
                <Button variant="outline" size="sm" className="h-7 text-xs" onClick={selectAll}>
                  {t("trash.selectAll")}
                </Button>
              )}
              <Button
                variant="destructive"
                size="sm"
                className="h-7 text-xs"
                disabled={isEmpty}
                onClick={() => setPurgeTarget("all")}
              >
                <Trash2 className="mr-1 size-3.5" />
                {t("trash.emptyTrash")}
              </Button>
            </>
          )}
        </div>
      </div>

      {/* 保留策略提示 */}
      <div className="flex shrink-0 items-center gap-1.5 border-b border-border/30 bg-muted/30 px-4 py-1.5 text-[11px] text-foreground/55">
        <TriangleAlert className="size-3.5 shrink-0" />
        <span>
          {autoPurgeEnabled
            ? t("trash.retentionHint", { days: retentionDays })
            : t("trash.retentionDisabledHint")}
        </span>
      </div>

      {/* 内容区 */}
      <div className={cn("min-h-0 flex-1", isEmpty ? "overflow-y-auto p-4" : "overflow-hidden")}>
        {trashLoading && isEmpty ? (
          <div className="flex h-full items-center justify-center">
            <p className="text-sm text-foreground/50">{t("trash.loading")}</p>
          </div>
        ) : isEmpty ? (
          <div className="flex h-full items-center justify-center">
            <div className="flex flex-col items-center gap-3 text-foreground/30">
              <Trash2 className="size-12" strokeWidth={1} />
              <p className="text-sm">{t("trash.empty")}</p>
            </div>
          </div>
        ) : (
          <VirtualGrid
            items={trashed}
            getKey={(wp) => wp.id}
            className="h-full p-4"
            renderVersion={selectedCount}
            renderItem={(wp) => (
              <TrashCard
                wallpaper={wp}
                remainingDays={
                  autoPurgeEnabled ? calcRemainingDays(wp.deleted_at, retentionDays) : null
                }
                selected={selectedIds.has(wp.id)}
                onToggle={toggleSelect}
                onRestore={(id) => handleRestore([id])}
                onPurge={(id) => setPurgeTarget([id])}
              />
            )}
          />
        )}
      </div>

      {/* 彻底删除 / 清空确认 */}
      <AlertDialog open={purgeTarget !== null} onOpenChange={() => setPurgeTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {purgeTarget === "all" ? t("trash.emptyConfirmTitle") : t("trash.purgeConfirmTitle")}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {purgeTarget === "all"
                ? t("trash.emptyConfirmDesc", { count: trashed.length })
                : t("trash.purgeConfirmDesc", {
                    count: Array.isArray(purgeTarget) ? purgeTarget.length : 0,
                  })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("trash.cancel")}</AlertDialogCancel>
            <AlertDialogAction
              onClick={handlePurgeConfirm}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              {t("trash.confirmPurge")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
};

export default TrashView;
