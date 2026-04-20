import { convertFileSrc } from "@tauri-apps/api/core";
import { addWallpapers } from "@/api/collectionWallpaper";
import { Check, Film, Image, List } from "lucide-react";
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { useWallpaperStore } from "@/stores/wallpaperStore";
import type { Wallpaper } from "@/stores/wallpaperStore";
import {
  Sheet,
  SheetContent,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";

interface WallpaperPickerDrawerProps {
  open: boolean;
  collectionId: number;
  /** 收藏夹中已有的壁纸 ID，用于过滤 */
  existingWallpaperIds: Set<number>;
  onClose: () => void;
  onConfirm: () => void;
}

const WallpaperPickerDrawer: React.FC<WallpaperPickerDrawerProps> = ({
  open,
  collectionId,
  existingWallpaperIds,
  onClose,
  onConfirm,
}) => {
  const allWallpapers = useWallpaperStore((s) => s.wallpapers);
  const { t } = useTranslation();
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set());
  const [submitting, setSubmitting] = useState(false);

  // 可选壁纸：排除已在收藏夹中的
  const availableWallpapers = allWallpapers.filter((w) => !existingWallpaperIds.has(w.id));

  const toggleSelect = useCallback((id: number) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const selectAll = useCallback(() => {
    setSelectedIds(new Set(availableWallpapers.map((w) => w.id)));
  }, [availableWallpapers]);

  const clearSelection = useCallback(() => {
    setSelectedIds(new Set());
  }, []);

  const handleConfirm = useCallback(async () => {
    if (selectedIds.size === 0) return;
    setSubmitting(true);
    try {
      await addWallpapers(collectionId, Array.from(selectedIds));
      setSelectedIds(new Set());
      onConfirm();
    } catch (e) {
      console.error("[addWallpapersToCollection]", e);
    } finally {
      setSubmitting(false);
    }
  }, [selectedIds, collectionId, onConfirm]);

  const handleClose = useCallback(() => {
    setSelectedIds(new Set());
    onClose();
  }, [onClose]);

  return (
    <Sheet open={open} onOpenChange={handleClose}>
      <SheetContent className="flex w-80 flex-col gap-0 p-0 sm:w-96">
        <SheetHeader className="border-b border-border px-4 py-3">
          <SheetTitle className="flex items-center gap-2 text-base">
            <List className="size-4" />
            {t("picker.title")}
          </SheetTitle>
        </SheetHeader>

        {/* 操作栏 */}
        <div className="flex items-center gap-2 border-b border-border px-4 py-2 text-sm">
          <span className="text-muted-foreground">
            {selectedIds.size} / {availableWallpapers.length}
          </span>
          <button type="button" onClick={selectAll} className="text-primary hover:underline">
            {t("picker.selectAll")}
          </button>
          <button type="button" onClick={clearSelection} className="text-primary hover:underline">
            {t("picker.clear")}
          </button>
        </div>

        {/* 壁纸列表 */}
        <ScrollArea className="flex-1">
          {availableWallpapers.length === 0 ? (
            <div className="flex h-40 items-center justify-center text-sm text-muted-foreground">
              {t("picker.allAdded")}
            </div>
          ) : (
            <div className="divide-y divide-border">
              {availableWallpapers.map((wp) => (
                <PickerRow
                  key={wp.id}
                  wallpaper={wp}
                  selected={selectedIds.has(wp.id)}
                  onClick={() => toggleSelect(wp.id)}
                />
              ))}
            </div>
          )}
        </ScrollArea>

        {/* 底部操作 */}
        <SheetFooter className="border-t border-border px-4 py-3">
          <div className="flex w-full gap-2">
            <Button variant="outline" className="flex-1" onClick={handleClose}>
              {t("picker.cancel")}
            </Button>
            <Button
              className="flex-1"
              onClick={handleConfirm}
              disabled={selectedIds.size === 0 || submitting}
            >
              {submitting
                ? t("picker.adding")
                : selectedIds.size > 0 ? t("picker.addCount", { count: selectedIds.size }) : t("picker.add")}
            </Button>
          </div>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
};

/** 列表行：缩略图 + 文件名 + 选中状态 */
const PickerRow: React.FC<{
  wallpaper: Wallpaper;
  selected: boolean;
  onClick: () => void;
}> = ({ wallpaper, selected, onClick }) => {
  const { t } = useTranslation();
  const TypeIcon = wallpaper.type === "video" ? Film : Image;

  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex w-full items-center gap-3 px-4 py-2 text-left transition-colors hover:bg-accent/50",
        selected && "bg-primary/5",
      )}
    >
      {/* 选中指示器 */}
      <div
        className={cn(
          "flex size-5 shrink-0 items-center justify-center rounded-full border-2 transition-colors",
          selected
            ? "border-primary bg-primary text-primary-foreground"
            : "border-muted-foreground/30",
        )}
      >
        {selected && <Check className="size-3" />}
      </div>

      {/* 缩略图 */}
      <div className="relative size-10 shrink-0 overflow-hidden rounded bg-muted">
        {wallpaper.thumb_path ? (
          <img
            src={convertFileSrc(wallpaper.thumb_path)}
            alt={wallpaper.name}
            className="size-full object-cover"
            loading="lazy"
          />
        ) : (
          <div className="flex size-full items-center justify-center">
            <TypeIcon className="size-4 text-muted-foreground/40" />
          </div>
        )}
        {/* 类型角标 */}
        {wallpaper.type !== "image" && (
          <div className="absolute bottom-0 right-0 rounded-tl bg-black/60 px-0.5 py-px text-[8px] leading-none text-white">
            {wallpaper.type === "video" ? "MP4" : "GIF"}
          </div>
        )}
      </div>

      {/* 文件信息 */}
      <div className="min-w-0 flex-1">
        <p className="truncate text-sm text-foreground">{wallpaper.name}</p>
        <p className="text-xs text-muted-foreground">
          {wallpaper.type === "video" ? t("preview.video") : wallpaper.type === "gif" ? t("preview.gif") : t("preview.image")}
          {wallpaper.width && wallpaper.height && ` · ${wallpaper.width}×${wallpaper.height}`}
        </p>
      </div>
    </button>
  );
};

export default WallpaperPickerDrawer;
