import { memo, useCallback, useEffect, useState, type FC } from "react";
import { useTranslation } from "react-i18next";
import { Layers, Monitor, Pencil, Play, RefreshCw, Save, Trash2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import { DialogContent, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { toast } from "@/components/ui/toast";
import { formatInterval } from "@/hooks/useMonitorConfig";
import { useCollectionStore } from "@/stores/collectionStore";
import { useSceneStore } from "@/stores/sceneStore";
import { useWallpaperStore } from "@/stores/wallpaperStore";
import type { SceneMonitorState } from "@/api/config";

/**
 * 场景快照弹窗：左右分栏，左栏陈列当前桌面状态（并保存），右栏管理已保存场景。
 *
 * 两栏各自独立滚动：列表区 min-h-0 + overflow-y-auto，父容器给确定高度，
 * 避免显示器多 / 场景多时把弹窗撑出屏幕。
 *
 * 展示用的壁纸名 / 收藏夹名按 id 从已有 store 查，不写进快照——
 * 改名后仍显示新名字，不会被保存时的名称固化。
 */
const SceneDialog: FC = memo(() => {
  const { t } = useTranslation();
  const { current, scenes, loading, refresh, save, rename, remove, apply } = useSceneStore();
  const wallpapers = useWallpaperStore((s) => s.wallpapers);
  const collections = useCollectionStore((s) => s.collections);

  const [name, setName] = useState("");
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editingName, setEditingName] = useState("");

  // Radix Dialog 关闭时会卸载内容，因此每次打开都会重新拉取一次。
  useEffect(() => {
    void refresh();
  }, [refresh]);

  const wallpaperName = useCallback(
    (id: number | null) => {
      if (id === null) return t("scene.noWallpaper");
      return wallpapers.find((w) => w.id === id)?.name ?? t("scene.unknownWallpaper");
    },
    [wallpapers, t],
  );

  const collectionName = useCallback(
    (id: number | null) => {
      if (id === null) return null;
      return collections.find((c) => c.id === id)?.name ?? t("scene.unknownCollection");
    },
    [collections, t],
  );

  const handleSave = useCallback(async () => {
    const trimmed = name.trim();
    if (!trimmed) {
      toast.error(t("scene.nameRequired"));
      return;
    }
    try {
      const saved = await save(trimmed);
      setName("");
      toast.success(t("scene.saved", { name: saved.name }));
    } catch {
      // invoke 已统一弹出错误提示
      return;
    }
  }, [name, save, t]);

  const handleApply = useCallback(
    async (id: number, sceneName: string) => {
      try {
        const result = await apply(id);
        if (result.skipped.length > 0) {
          toast.warning(
            `${t("scene.appliedPartial", { name: sceneName })}：${result.skipped.join("；")}`,
          );
        } else {
          toast.success(t("scene.appliedOk", { name: sceneName, count: result.applied }));
        }
      } catch {
        return;
      }
    },
    [apply, t],
  );

  const handleRename = useCallback(
    async (id: number) => {
      const trimmed = editingName.trim();
      if (!trimmed) return;
      try {
        await rename(id, trimmed);
        setEditingId(null);
        toast.success(t("scene.renamed"));
      } catch {
        return;
      }
    },
    [editingName, rename, t],
  );

  const handleDelete = useCallback(
    async (id: number) => {
      try {
        await remove(id);
        toast.success(t("scene.deleted"));
      } catch {
        return;
      }
    },
    [remove, t],
  );

  const displayModeLabel = (mode: string) => {
    const keys: Record<string, string> = {
      independent: "monitor.displayIndependent",
      mirror: "monitor.displayMirror",
      extend: "monitor.displayExtend",
    };
    return keys[mode] ? t(keys[mode]) : mode;
  };

  const renderMonitor = (monitor: SceneMonitorState) => {
    const boundCollection = collectionName(monitor.collectionId);
    return (
      <div
        key={monitor.monitorId}
        className="rounded-lg border border-border/50 bg-card px-3 py-2.5"
      >
        <div className="mb-1.5 flex items-center gap-1.5 text-xs font-medium text-foreground">
          <Monitor className="size-3.5 shrink-0 text-primary" />
          <span className="truncate">{t("scene.monitor", { id: monitor.monitorId })}</span>
        </div>
        <div className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-[11px]">
          <span className="text-muted-foreground">{t("scene.wallpaper")}</span>
          <span className="truncate text-foreground/80">{wallpaperName(monitor.wallpaperId)}</span>
          {boundCollection && (
            <>
              <span className="text-muted-foreground">{t("scene.collection")}</span>
              <span className="truncate text-foreground/80">{boundCollection}</span>
            </>
          )}
          <span className="text-muted-foreground">{t("scene.fitMode")}</span>
          <span className="text-foreground/80">{monitor.fitMode}</span>
          <span className="text-muted-foreground">{t("scene.playMode")}</span>
          <span className="text-foreground/80">
            {monitor.playMode === "random" ? t("monitor.random") : t("monitor.sequential")}
          </span>
          <span className="text-muted-foreground">{t("scene.interval")}</span>
          <span className="text-foreground/80">
            {formatInterval(monitor.playInterval, t)}
            <span className="ml-1.5 text-muted-foreground">
              · {monitor.isEnabled ? t("scene.enabled") : t("scene.paused")}
            </span>
          </span>
        </div>
      </div>
    );
  };

  return (
    <DialogContent className="gap-0 overflow-hidden rounded-xl border-border/50 p-0 sm:max-w-[860px]">
      <div className="border-b border-border/40 px-6 py-3">
        <DialogTitle className="flex items-center gap-2 text-base font-semibold">
          <Layers className="size-4 text-primary" />
          {t("scene.title")}
        </DialogTitle>
        <p className="mt-1 text-[11px] leading-relaxed text-muted-foreground">{t("scene.desc")}</p>
      </div>

      {/* 固定高度 + 两栏各自 min-h-0/overflow，内容再多也不会撑破弹窗 */}
      <div className="flex h-[560px] max-h-[70vh]">
        {/* ===== 左：当前桌面状态 + 保存 ===== */}
        <div className="flex min-w-0 flex-1 flex-col border-r border-border/40">
          <div className="flex items-center justify-between px-5 pt-4 pb-2">
            <h3 className="text-sm font-medium text-foreground">{t("scene.currentTitle")}</h3>
            <Button
              variant="ghost"
              size="xs"
              onClick={() => void refresh()}
              disabled={loading}
              title={t("scene.refresh")}
            >
              <RefreshCw className="size-3.5" />
            </Button>
          </div>
          <div className="mx-5 rounded-lg border border-border/50 bg-card px-3 py-2 text-[11px]">
            <div className="flex flex-wrap gap-x-4 gap-y-1">
              <span className="text-muted-foreground">
                {t("scene.displayMode")}：
                <span className="text-foreground/80">
                  {current ? displayModeLabel(current.displayMode) : "—"}
                </span>
              </span>
              <span className="text-muted-foreground">
                {t("scene.volume")}：
                <span className="text-foreground/80">{current?.globalVolume ?? "—"}</span>
              </span>
            </div>
          </div>

          <div className="min-h-0 flex-1 space-y-2 overflow-y-auto px-5 py-3">
            {current?.monitors.length ? (
              current.monitors.map(renderMonitor)
            ) : (
              <p className="rounded-lg border border-dashed border-border/50 px-3 py-4 text-center text-xs text-muted-foreground">
                {t("scene.emptyCurrent")}
              </p>
            )}
          </div>

          {/* 保存区固定在左栏底部，不随列表滚动 */}
          <div className="space-y-2 border-t border-border/40 px-5 py-3">
            <h3 className="text-sm font-medium text-foreground">{t("scene.saveTitle")}</h3>
            <div className="flex gap-2">
              <Input
                value={name}
                onChange={(e) => setName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void handleSave();
                }}
                placeholder={t("scene.namePlaceholder")}
                maxLength={32}
                className="h-8 text-xs"
              />
              <Button size="sm" onClick={() => void handleSave()} disabled={loading}>
                <Save className="size-3.5" />
                {t("scene.save")}
              </Button>
            </div>
            <p className="text-[11px] text-muted-foreground">{t("scene.overwriteHint")}</p>
          </div>
        </div>

        {/* ===== 右：已保存的场景 ===== */}
        <div className="flex min-w-0 flex-1 flex-col">
          <div className="px-5 pt-4 pb-3">
            <h3 className="text-sm font-medium text-foreground">{t("scene.listTitle")}</h3>
          </div>
          <div className="min-h-0 flex-1 space-y-2 overflow-y-auto px-5 pb-4">
            {scenes.length === 0 ? (
              <p className="rounded-lg border border-dashed border-border/50 px-3 py-4 text-center text-xs text-muted-foreground">
                {t("scene.empty")}
              </p>
            ) : (
              scenes.map((scene) => (
                <div
                  key={scene.id}
                  className="rounded-lg border border-border/50 bg-card px-3 py-2.5"
                >
                  {editingId === scene.id ? (
                    <div className="flex gap-2">
                      <Input
                        autoFocus
                        value={editingName}
                        onChange={(e) => setEditingName(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") void handleRename(scene.id);
                          if (e.key === "Escape") setEditingId(null);
                        }}
                        maxLength={32}
                        className="h-7 text-xs"
                      />
                      <Button size="xs" onClick={() => void handleRename(scene.id)}>
                        {t("scene.confirmRename")}
                      </Button>
                    </div>
                  ) : (
                    <div className="flex items-center gap-1.5">
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-xs font-medium text-foreground">{scene.name}</p>
                        <p className="text-[11px] text-muted-foreground">
                          {t("scene.monitorCount", { count: scene.state.monitors.length })}
                        </p>
                      </div>
                      <Button
                        size="xs"
                        onClick={() => void handleApply(scene.id, scene.name)}
                        title={t("scene.apply")}
                      >
                        <Play className="size-3" />
                        {t("scene.apply")}
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon-xs"
                        onClick={() => {
                          setEditingId(scene.id);
                          setEditingName(scene.name);
                        }}
                        title={t("scene.rename")}
                      >
                        <Pencil className="size-3" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon-xs"
                        onClick={() => void handleDelete(scene.id)}
                        title={t("scene.delete")}
                      >
                        <Trash2 className="size-3" />
                      </Button>
                    </div>
                  )}
                </div>
              ))
            )}
          </div>
        </div>
      </div>
    </DialogContent>
  );
});

SceneDialog.displayName = "SceneDialog";

export default SceneDialog;
