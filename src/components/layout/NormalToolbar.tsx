import { memo, useCallback, useMemo, useState, type FC } from "react";
import { Move, Plus, RefreshCw, Search, Settings2, Sparkles, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Dialog, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import WallpaperPickerDialog from "@/components/wallpaper/WallpaperPickerDialog";
import type { Wallpaper } from "@/api/config";

interface NormalToolbarProps {
  isCollectionView: boolean;
  /** 当前收藏夹是否为智能收藏夹（成员按规则实时求值，不支持手动排序） */
  isSmartCollection: boolean;
  /** 监听夹：成员由目录扫描独占，禁止新增、提供同步、管理走源文件删除 */
  isWatchedCollection: boolean;
  isEmpty: boolean;
  searchExpanded: boolean;
  normalKeyword: string;
  /** 收藏夹 ID（仅收藏夹视图有效） */
  collectionId: number | null;
  /** 收藏夹中已有的壁纸列表（用于 picker 禁用已添加项） */
  collectionWallpapers: Wallpaper[];
  onRefresh: () => void;
  /** 监听夹手动同步（触发后端强制扫描并刷新视图） */
  onSync: () => void;
  onSearchExpand: () => void;
  onSearchCollapse: () => void;
  onNormalKeywordChange: (value: string) => void;
  onEnterSortMode: () => void;
  onEnterManageMode: () => void;
  /** 智能收藏夹：打开规则编辑对话框（替代壁纸管理模式） */
  onEditSmartRule: () => void;
  /** picker 确认后的回调 */
  onPickerConfirm: () => void;
}

/**
 * 常态模式下的操作栏。
 *
 * 采用显式三槽布局：左操作区 / 弹性留白 + 搜索区 / 右操作区。
 * 三个槽位都是恒定存在的 flex 容器，视图切换时只替换槽内内容、不增删槽位本身，
 * 从而保证「管理」等主操作按钮的横向位置在主库、手动收藏夹、智能收藏夹三种视图下一致，
 * 不再因左侧按钮宽度差异或中间按钮的条件渲染而漂移。
 */
const NormalToolbar: FC<NormalToolbarProps> = memo(
  ({
    isCollectionView,
    isSmartCollection,
    isWatchedCollection,
    isEmpty,
    searchExpanded,
    normalKeyword,
    collectionId,
    collectionWallpapers,
    onRefresh,
    onSync,
    onSearchExpand,
    onSearchCollapse,
    onNormalKeywordChange,
    onEnterSortMode,
    onEnterManageMode,
    onEditSmartRule,
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

    const handleKeywordInput = useCallback(
      (e: React.ChangeEvent<HTMLInputElement>) => {
        onNormalKeywordChange(e.target.value);
      },
      [onNormalKeywordChange],
    );

    const handleSearchBlur = useCallback(() => {
      if (!normalKeyword) onSearchCollapse();
    }, [normalKeyword, onSearchCollapse]);

    const handleSearchClear = useCallback(() => {
      onNormalKeywordChange("");
      onSearchCollapse();
    }, [onNormalKeywordChange, onSearchCollapse]);

    const handleSearchKeyDown = useCallback(
      (e: React.KeyboardEvent<HTMLInputElement>) => {
        if (e.key === "Escape") handleSearchClear();
      },
      [handleSearchClear],
    );

    // 已存在于收藏夹中的壁纸 ID 集合
    const existingWallpaperIds = useMemo(
      () => new Set(collectionWallpapers.map((w) => w.id)),
      [collectionWallpapers],
    );

    return (
      <>
        {/* ===== 槽位一：左操作区（视图级动作，统一为定宽图标按钮） ===== */}
        <div className="flex shrink-0 items-center gap-1">
          {isCollectionView ? (
            isWatchedCollection ? (
              /* 监听夹成员由扫描独占：没有「新增」，只提供手动同步 */
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={onSync}
                className="text-muted-foreground hover:bg-primary-hover hover:text-foreground"
                title={t("watched.rescan")}
                aria-label={t("watched.rescan")}
              >
                <RefreshCw className="size-3.5" />
              </Button>
            ) : collectionId !== null && !isSmartCollection ? (
              <Dialog open={pickerOpen} onOpenChange={handlePickerOpenChange}>
                <DialogTrigger asChild>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    className="text-muted-foreground hover:bg-primary-hover hover:text-foreground"
                    title={t("main.addWallpaper")}
                    aria-label={t("main.addWallpaper")}
                  >
                    <Plus className="size-3.5" />
                  </Button>
                </DialogTrigger>
                <WallpaperPickerDialog
                  collectionId={collectionId}
                  existingWallpaperIds={existingWallpaperIds}
                  onClose={() => setPickerOpen(false)}
                  onConfirm={handlePickerConfirm}
                />
              </Dialog>
            ) : null
          ) : (
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={onRefresh}
              className="text-muted-foreground hover:bg-primary-hover hover:text-foreground"
              title={t("main.refresh")}
              aria-label={t("main.refresh")}
            >
              <RefreshCw className="size-3.5" />
            </Button>
          )}
        </div>

        {/* 弹性留白：把搜索区与右操作区一起推到右侧 */}
        <div className="min-w-0 flex-1" />

        {/* ===== 槽位二：搜索区（折叠态与展开态共用同一槽位） ===== */}
        <div className="flex shrink-0 items-center">
          {isEmpty ? null : searchExpanded ? (
            <div className="relative animate-in fade-in slide-in-from-right-2 duration-200">
              <Search className="absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-foreground/40" />
              <Input
                autoFocus
                value={normalKeyword}
                onChange={handleKeywordInput}
                placeholder={t("grid.searchPlaceholder")}
                className="h-8 w-44 pl-7 pr-7 text-xs"
                onBlur={handleSearchBlur}
                onKeyDown={handleSearchKeyDown}
              />
              {normalKeyword && (
                <button
                  type="button"
                  onClick={handleSearchClear}
                  className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded-sm p-0.5 text-foreground/40 hover:text-foreground"
                  title={t("grid.clearSearch")}
                  aria-label={t("grid.clearSearch")}
                >
                  <X className="size-3" />
                </button>
              )}
            </div>
          ) : (
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={onSearchExpand}
              className="text-muted-foreground hover:bg-primary-hover hover:text-foreground"
              title={t("grid.search")}
              aria-label={t("grid.search")}
            >
              <Search className="size-3.5" />
            </Button>
          )}
        </div>

        {/* ===== 槽位三：右操作区（次操作 + 主操作，主操作恒定居于末尾） ===== */}
        <div className="flex shrink-0 items-center gap-1">
          {!isEmpty && isCollectionView && !isSmartCollection ? (
            <Button
              variant="ghost"
              size="sm"
              onClick={onEnterSortMode}
              className="gap-1.5 text-muted-foreground hover:bg-primary-hover hover:text-foreground"
            >
              <Move className="size-3.5" />
              {t("main.sortWallpapers")}
            </Button>
          ) : !isEmpty ? (
            <span
              className="truncate whitespace-nowrap text-xs text-foreground/40"
              title={!isCollectionView ? t("main.localNoSort") : t("main.smartNoSort")}
            >
              {!isCollectionView ? t("main.localNoSort") : t("main.smartNoSort")}
            </span>
          ) : null}

          {/* 主操作位：智能收藏夹为「编辑规则」，其余为「管理壁纸」。
              智能收藏夹空结果时同样要显示，因为此时正是需要修正规则的场景。 */}
          {isSmartCollection ? (
            <Button
              variant="ghost"
              size="sm"
              onClick={onEditSmartRule}
              className="gap-1.5 text-muted-foreground hover:bg-primary-hover hover:text-foreground"
            >
              <Sparkles className="size-3.5" />
              {t("sidebar.editRule")}
            </Button>
          ) : !isEmpty ? (
            <Button
              variant="ghost"
              size="sm"
              onClick={onEnterManageMode}
              className="gap-1.5 text-muted-foreground hover:bg-primary-hover hover:text-foreground"
            >
              <Settings2 className="size-3.5" />
              {t("main.manageWallpapers")}
            </Button>
          ) : (
            <div className="h-8" aria-hidden />
          )}
        </div>
      </>
    );
  },
);

NormalToolbar.displayName = "NormalToolbar";

export default NormalToolbar;
