import { Loader2, Plus, Minus } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  getTags,
  getWallpaperTags,
  setWallpaperTags,
  tagWallpapers,
  untagWallpapers,
} from "@/api/tag";
import type { Tag, TagWithCount } from "@/api/config";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { toast } from "@/components/ui/toast";
import { TagInput } from "@/components/wallpaper/TagInput";

// ============ 类型定义 ============

export interface TagEditorDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /**
   * single：单张壁纸，覆盖式编辑（setWallpaperTags）
   * batch：一批壁纸，新增（tagWallpapers）/ 移除（untagWallpapers）
   */
  mode: "single" | "batch";
  /** 目标壁纸 id：single 传 1 个，batch 传选中集 */
  wallpaperIds: number[];
  /** 保存成功后回调（可用于刷新命中/提示） */
  onSaved?: () => void;
}

/**
 * 标签编辑对话框
 *
 * - single：加载该壁纸现有标签 → TagInput 覆盖编辑 → setWallpaperTags 覆盖保存
 * - batch：两个 TagInput，一个添加、一个移除，分别调用 tagWallpapers / untagWallpapers
 */
export const TagEditorDialog: React.FC<TagEditorDialogProps> = ({
  open,
  onOpenChange,
  mode,
  wallpaperIds,
  onSaved,
}) => {
  const { t } = useTranslation();

  const [allTags, setAllTags] = useState<TagWithCount[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);

  // single：覆盖式标签集合
  const [singleTags, setSingleTags] = useState<string[]>([]);
  // batch：待添加 / 待移除
  const [addTags, setAddTags] = useState<string[]>([]);
  const [removeTags, setRemoveTags] = useState<string[]>([]);

  const suggestions = allTags.map((tenta) => tenta.name);

  // 打开时加载数据
  useEffect(() => {
    if (!open) return;
    let cancelled = false;

    const load = async () => {
      setLoading(true);
      try {
        const [tags, current] = await Promise.all([
          getTags(),
          mode === "single" && wallpaperIds.length === 1
            ? getWallpaperTags(wallpaperIds[0])
            : Promise.resolve<Tag[]>([]),
        ]);
        if (cancelled) return;
        setAllTags(tags);
        setSingleTags(current.map((c) => c.name));
        setAddTags([]);
        setRemoveTags([]);
      } catch (e) {
        console.error("[TagEditorDialog.load]", e);
        toast.error(t("tags.loadFailed"));
      } finally {
        if (!cancelled) setLoading(false);
      }
    };

    load();
    return () => {
      cancelled = true;
    };
  }, [open, mode, wallpaperIds, t]);

  const handleSave = useCallback(async () => {
    if (wallpaperIds.length === 0) return;
    setSaving(true);
    try {
      if (mode === "single") {
        await setWallpaperTags(wallpaperIds[0], singleTags);
        toast.success(t("tags.saved"));
      } else {
        let added = 0;
        let removed = 0;
        if (addTags.length > 0) {
          added = await tagWallpapers(wallpaperIds, addTags);
        }
        if (removeTags.length > 0) {
          // 移除需按 id：从 allTags 解析已存在标签的 id（不存在的移除名忽略）
          const idByName = new Map(allTags.map((tenta) => [tenta.name, tenta.id]));
          const removeIds = removeTags
            .map((name) => idByName.get(name))
            .filter((id): id is number => id != null);
          if (removeIds.length > 0) {
            removed = await untagWallpapers(wallpaperIds, removeIds);
          }
        }
        toast.success(t("tags.batchApplied", { added, removed }));
      }
      onSaved?.();
      onOpenChange(false);
    } catch (e) {
      console.error("[TagEditorDialog.save]", e);
      toast.error(t("tags.saveFailed"));
    } finally {
      setSaving(false);
    }
  }, [mode, wallpaperIds, singleTags, addTags, removeTags, allTags, onSaved, onOpenChange, t]);

  const isBatch = mode === "batch";
  const nothingToDo = isBatch
    ? addTags.length === 0 && removeTags.length === 0
    : false;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{isBatch ? t("tags.batchTitle") : t("tags.editTitle")}</DialogTitle>
          <DialogDescription>
            {isBatch
              ? t("tags.batchDesc", { count: wallpaperIds.length })
              : t("tags.editDesc")}
          </DialogDescription>
        </DialogHeader>

        {loading ? (
          <div className="flex items-center justify-center py-8 text-foreground/50">
            <Loader2 className="size-5 animate-spin" />
          </div>
        ) : isBatch ? (
          <div className="flex flex-col gap-4 py-1">
            <div className="flex flex-col gap-1.5">
              <label className="flex items-center gap-1.5 text-xs font-medium text-foreground/70">
                <Plus className="size-3.5 text-primary" />
                {t("tags.addLabel")}
              </label>
              <TagInput
                value={addTags}
                onChange={setAddTags}
                suggestions={suggestions}
                placeholder={t("tags.addPlaceholder")}
                autoFocus
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <label className="flex items-center gap-1.5 text-xs font-medium text-foreground/70">
                <Minus className="size-3.5 text-destructive" />
                {t("tags.removeLabel")}
              </label>
              <TagInput
                value={removeTags}
                onChange={setRemoveTags}
                suggestions={suggestions}
                placeholder={t("tags.removePlaceholder")}
              />
            </div>
          </div>
        ) : (
          <div className="flex flex-col gap-1.5 py-1">
            <TagInput
              value={singleTags}
              onChange={setSingleTags}
              suggestions={suggestions}
              placeholder={t("tags.inputPlaceholder")}
              autoFocus
            />
            <p className="text-xs text-foreground/40">{t("tags.editHint")}</p>
          </div>
        )}

        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={saving}>
            {t("tags.cancel")}
          </Button>
          <Button onClick={handleSave} disabled={saving || loading || nothingToDo}>
            {saving && <Loader2 className="size-4 animate-spin" />}
            {t("tags.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};

export default TagEditorDialog;
