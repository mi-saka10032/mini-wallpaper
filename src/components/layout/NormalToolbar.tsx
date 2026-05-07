import React, { useCallback, useMemo, useState } from "react";
import { GripVertical, Plus, RefreshCw, Search, Settings2, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Dialog, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import WallpaperPickerDialog from "@/components/wallpaper/WallpaperPickerDialog";
import type { Wallpaper } from "@/api/config";

interface NormalToolbarProps {
  isCollectionView: boolean;
  isEmpty: boolean;
  searchExpanded: boolean;
  normalKeyword: string;
  /** 收藏夹 ID（仅收藏夹视图有效） */
  collectionId: number | null;
  /** 收藏夹中已有的壁纸列表（用于 picker 禁用已添加项） */
  collectionWallpapers: Wallpaper[];
  onRefresh: () => void;
  onSearchExpand: () => void;
  onSearchCollapse: () => void;
  onNormalKeywordChange: (value: string) => void;
  onEnterSortMode: () => void;
  onEnterManageMode: () => void;
  /** picker 确认后的回调 */
  onPickerConfirm: () => void;
}

/** 常态模式下的操作栏 */
const NormalToolbar: React.FC<NormalToolbarProps> = React.memo(({
  isCollectionView,
  isEmpty,
  searchExpanded,
  normalKeyword,
  collectionId,
  collectionWallpapers,
  onRefresh,
  onSearchExpand,
  onSearchCollapse,
  onNormalKeywordChange,
  onEnterSortMode,
  onEnterManageMode,
  onPickerConfirm,
}) => {
  const { t } = useTranslation();
  const [pickerOpen, setPickerOpen] = useState(false);

  const handlePickerConfirm = useCallback(() => {
    setPickerOpen(false);
    onPickerConfirm();
  }, [onPickerConfirm]);

  const handlePickerOpenChange = useCallback((open: boolean) => {
    setPickerOpen(open);
  }, []);

  // 已存在于收藏夹中的壁纸 ID 集合
  const existingWallpaperIds = useMemo(
    () => new Set(collectionWallpapers.map((w) => w.id)),
    [collectionWallpapers],
  );

  return (
    <>
      {/* 收藏夹视图：添加壁纸按钮 + Dialog */}
      {isCollectionView && collectionId !== null && (
        <Dialog open={pickerOpen} onOpenChange={handlePickerOpenChange}>
          <DialogTrigger asChild>
            <Button
              variant="ghost"
              size="sm"
              className="gap-1.5 text-foreground/60 hover:text-foreground hover:bg-primary-hover"
            >
              <Plus className="size-3.5" />
              {t("main.addWallpaper")}
            </Button>
          </DialogTrigger>
          <WallpaperPickerDialog
            collectionId={collectionId}
            existingWallpaperIds={existingWallpaperIds}
            onClose={() => setPickerOpen(false)}
            onConfirm={handlePickerConfirm}
          />
        </Dialog>
      )}

      {/* 本地壁纸视图：刷新按钮 */}
      {!isCollectionView && (
        <Button
          variant="ghost"
          size="sm"
          onClick={onRefresh}
          className="gap-1.5 text-foreground/60 hover:text-foreground hover:bg-primary-hover"
          title={t("main.refresh")}
        >
          <RefreshCw className="size-3.5" />
        </Button>
      )}

      <div className="flex-1" />

      {/* 常态搜索框（可折叠） */}
      {!isEmpty && (
        <div className="flex items-center">
          {searchExpanded ? (
            <div className="relative animate-in fade-in slide-in-from-right-2 duration-200">
              <Search className="absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-foreground/40" />
              <Input
                autoFocus
                value={normalKeyword}
                onChange={(e) => onNormalKeywordChange(e.target.value)}
                placeholder={t("grid.searchPlaceholder")}
                className="h-7 w-44 pl-7 pr-7 text-xs"
                onBlur={() => {
                  if (!normalKeyword) onSearchCollapse();
                }}
                onKeyDown={(e) => {
                  if (e.key === "Escape") {
                    onNormalKeywordChange("");
                    onSearchCollapse();
                  }
                }}
              />
              {normalKeyword && (
                <button
                  type="button"
                  onClick={() => {
                    onNormalKeywordChange("");
                    onSearchCollapse();
                  }}
                  className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded-sm p-0.5 text-foreground/40 hover:text-foreground"
                >
                  <X className="size-3" />
                </button>
              )}
            </div>
          ) : (
            <Button
              variant="ghost"
              size="sm"
              onClick={onSearchExpand}
              className="gap-1.5 text-foreground/60 hover:text-foreground hover:bg-primary-hover"
            >
              <Search className="size-3.5" />
            </Button>
          )}
        </div>
      )}

      {/* 收藏夹视图：拖拽排序按钮 */}
      {!isEmpty && isCollectionView && (
        <Button
          variant="ghost"
          size="sm"
          onClick={onEnterSortMode}
          className="gap-1.5 text-foreground/60 hover:text-foreground hover:bg-primary-hover"
        >
          <GripVertical className="size-3.5" />
          {t("main.sortWallpapers")}
        </Button>
      )}

      {!isEmpty && (
        <Button
          variant="ghost"
          size="sm"
          onClick={onEnterManageMode}
          className="gap-1.5 text-foreground/60 hover:text-foreground hover:bg-primary-hover"
        >
          <Settings2 className="size-3.5" />
          {t("main.manageWallpapers")}
        </Button>
      )}
    </>
  );
});

NormalToolbar.displayName = "NormalToolbar";

export default NormalToolbar;
