import { Check, ImagePlus } from "lucide-react";
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { useWallpaperStore } from "@/stores/wallpaperStore";
import { addWallpapers } from "@/api/collectionWallpaper";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import WallpaperGrid from "./WallpaperGrid";

interface WallpaperPickerDialogProps {
  open: boolean;
  collectionId: number;
  /** 收藏夹中已有的壁纸 ID，显示但禁用 */
  existingWallpaperIds: Set<number>;
  onClose: () => void;
  onConfirm: () => void;
}

const WallpaperPickerDialog: React.FC<WallpaperPickerDialogProps> = ({
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

  const handleOpenChange = useCallback(
    (isOpen: boolean) => {
      if (!isOpen) {
        setSelectedIds(new Set());
        onClose();
      }
    },
    [onClose],
  );

  // 可选壁纸数量（排除已在收藏夹中的）
  const availableCount = allWallpapers.filter((w) => !existingWallpaperIds.has(w.id)).length;

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent
        showCloseButton={false}
        className="flex h-[80vh] max-h-[800px] w-[80vw] max-w-[1200px] flex-col gap-0 overflow-hidden p-0"
      >
        {/* Header */}
        <DialogHeader className="shrink-0 border-b border-border px-6 py-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <DialogTitle className="flex items-center gap-2">
                <ImagePlus className="size-5" />
                {t("pickerDialog.title")}
              </DialogTitle>
              {selectedIds.size > 0 && (
                <div className="flex items-center gap-1.5 rounded-full bg-primary/10 px-3 py-1">
                  <Check className="size-3.5 text-primary" />
                  <span className="text-sm font-medium text-primary">
                    {t("pickerDialog.selectedCount", { count: selectedIds.size })}
                  </span>
                </div>
              )}
            </div>
            <DialogDescription className="sr-only">
              {t("pickerDialog.description")}
            </DialogDescription>
          </div>
        </DialogHeader>

        {/* Body: WallpaperGrid in select mode */}
        <div className="min-h-0 flex-1">
          <WallpaperGrid
            wallpapers={allWallpapers}
            mode="select"
            selectedIds={selectedIds}
            disabledIds={existingWallpaperIds}
            onSelectionChange={setSelectedIds}
            showFilter={true}
            className="h-full"
            emptyContent={
              availableCount === 0 ? (
                <div className="flex h-full min-h-40 flex-col items-center justify-center gap-2 text-muted-foreground">
                  <ImagePlus className="size-10" strokeWidth={1} />
                  <p className="text-sm">{t("pickerDialog.allAdded")}</p>
                </div>
              ) : undefined
            }
          />
        </div>

        {/* Footer: 固定操作栏 */}
        <div className="flex shrink-0 items-center justify-between border-t border-border px-6 py-3">
          <span className="text-sm text-muted-foreground">
            {selectedIds.size > 0
              ? t("pickerDialog.selectedHint", { count: selectedIds.size })
              : t("pickerDialog.hint")}
          </span>
          <div className="flex items-center gap-2">
            <Button variant="outline" onClick={() => handleOpenChange(false)}>
              {t("pickerDialog.cancel")}
            </Button>
            <Button
              onClick={handleConfirm}
              disabled={selectedIds.size === 0 || submitting}
            >
              {submitting
                ? t("pickerDialog.adding")
                : selectedIds.size > 0
                  ? t("pickerDialog.addCount", { count: selectedIds.size })
                  : t("pickerDialog.add")}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
};

export default WallpaperPickerDialog;
