import React from "react";
import { GripVertical, Plus, Search, Settings2, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

interface NormalToolbarProps {
  isCollectionView: boolean;
  isEmpty: boolean;
  searchExpanded: boolean;
  normalKeyword: string;
  onOpenPicker: () => void;
  onSearchExpand: () => void;
  onSearchCollapse: () => void;
  onNormalKeywordChange: (value: string) => void;
  onEnterSortMode: () => void;
  onEnterManageMode: () => void;
}

/** 常态模式下的操作栏 */
const NormalToolbar: React.FC<NormalToolbarProps> = React.memo(({
  isCollectionView,
  isEmpty,
  searchExpanded,
  normalKeyword,
  onOpenPicker,
  onSearchExpand,
  onSearchCollapse,
  onNormalKeywordChange,
  onEnterSortMode,
  onEnterManageMode,
}) => {
  const { t } = useTranslation();

  return (
    <>
      {/* 收藏夹视图：添加壁纸按钮 */}
      {isCollectionView && (
        <Button
          variant="ghost"
          size="sm"
          onClick={onOpenPicker}
          className="gap-1.5 text-foreground/60 hover:text-foreground hover:bg-foreground/5"
        >
          <Plus className="size-3.5" />
          {t("main.addWallpaper")}
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
              className="gap-1.5 text-foreground/60 hover:text-foreground hover:bg-foreground/5"
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
          className="gap-1.5 text-foreground/60 hover:text-foreground hover:bg-foreground/5"
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
          className="gap-1.5 text-foreground/60 hover:text-foreground hover:bg-foreground/5"
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
