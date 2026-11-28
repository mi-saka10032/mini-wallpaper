import { lazy, Suspense, useCallback, useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  FolderOpen,
  FolderSync,
  Heart,
  Plus,
  Sparkles,
  Star,
  Trash2,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import { Separator } from "@/components/ui/separator";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
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
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { TRASH_VIEW_ID } from "@/constants/views";
import { useCollectionStore, type Collection } from "@/stores/collectionStore";
import { memo, type FC } from "react";

const SortableCollectionList = lazy(() => import("./SortableCollectionList"));
const SmartCollectionDialog = lazy(() => import("@/components/wallpaper/SmartCollectionDialog"));

interface SidebarProps {
  activeId: number;
  onActiveIdChange: (id: number) => void;
}

const Sidebar: FC<SidebarProps> = memo(({ activeId, onActiveIdChange }) => {
  const { t } = useTranslation();
  const collections = useCollectionStore((s) => s.collections);
  const createCollection = useCollectionStore((s) => s.createCollection);
  const renameCollection = useCollectionStore((s) => s.renameCollection);
  const deleteCollection = useCollectionStore((s) => s.deleteCollection);

  // 新建/重命名 Dialog
  const [dialogMode, setDialogMode] = useState<"create" | "rename" | null>(null);
  const [dialogValue, setDialogValue] = useState("");
  const [dialogTarget, setDialogTarget] = useState<Collection | null>(null);
  const [dialogError, setDialogError] = useState("");

  // 删除确认
  const [deleteTarget, setDeleteTarget] = useState<Collection | null>(null);

  // 智能收藏夹规则对话框（创建 / 编辑）
  const [smartDialogOpen, setSmartDialogOpen] = useState(false);
  const [smartEditing, setSmartEditing] = useState<Collection | null>(null);

  // 手风琴折叠态放 localStorage（不进 DB、不与备份耦合）
  const [normalExpanded, setNormalExpanded] = useState(
    () => localStorage.getItem("sidebar.normalExpanded") !== "0",
  );
  const [watchedExpanded, setWatchedExpanded] = useState(
    () => localStorage.getItem("sidebar.watchedExpanded") !== "0",
  );
  const toggleNormal = useCallback(() => {
    setNormalExpanded((v) => {
      localStorage.setItem("sidebar.normalExpanded", v ? "0" : "1");
      return !v;
    });
  }, []);
  const toggleWatched = useCallback(() => {
    setWatchedExpanded((v) => {
      localStorage.setItem("sidebar.watchedExpanded", v ? "0" : "1");
      return !v;
    });
  }, []);

  // 三组各自独立：内置夹置顶、普通夹一组、监听夹一组。
  // 两个 SortableCollectionList 各自持有 DndContext，天然禁止跨组拖拽。
  const builtin = collections.find((c) => c.is_builtin === 1);
  const normalCollections = collections.filter((c) => c.is_builtin !== 1 && c.kind !== "watched");
  const watchedCollections = collections.filter((c) => c.kind === "watched");

  const openCreateSmart = useCallback(() => {
    setSmartEditing(null);
    setSmartDialogOpen(true);
  }, []);

  const openEditSmart = useCallback((collection: Collection) => {
    setSmartEditing(collection);
    setSmartDialogOpen(true);
  }, []);

  // 收藏夹列表由 App 的启动编排统一拉取；创建/删除/重命名后各自的 action
  // 仍会刷新，这里不再重复发起一次 getAll。

  // 打开新建 Dialog
  const openCreateDialog = useCallback(() => {
    setDialogMode("create");
    setDialogValue("");
    setDialogError("");
    setDialogTarget(null);
  }, []);

  // 打开重命名 Dialog
  const openRenameDialog = useCallback((collection: Collection) => {
    setDialogMode("rename");
    setDialogValue(collection.name);
    setDialogError("");
    setDialogTarget(collection);
  }, []);

  // 关闭 Dialog
  const closeDialog = useCallback(() => {
    setDialogMode(null);
    setDialogValue("");
    setDialogError("");
    setDialogTarget(null);
  }, []);

  // 确认 Dialog 操作
  const handleDialogConfirm = useCallback(async () => {
    const name = dialogValue.trim();
    if (!name) {
      setDialogError(t("sidebar.nameEmpty"));
      return;
    }

    if (dialogMode === "create") {
      await createCollection(name);
    } else if (dialogMode === "rename" && dialogTarget) {
      await renameCollection(dialogTarget.id, name);
    }

    closeDialog();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dialogMode, dialogValue, dialogTarget, createCollection, renameCollection]);

  // 删除确认
  const handleDelete = useCallback(async () => {
    if (!deleteTarget) return;
    if (activeId === deleteTarget.id) {
      onActiveIdChange(0);
    }
    await deleteCollection(deleteTarget.id);
    setDeleteTarget(null);
  }, [deleteTarget, activeId, onActiveIdChange, deleteCollection]);

  return (
    <div className="flex h-full w-52 shrink-0 flex-col overflow-hidden border-r border-border/50">
      {/* 顶部固定：壁纸库 + My Favorites 单独置顶 */}
      <div className="shrink-0 p-1">
        <div className="h-8">
          <button
            type="button"
            onClick={() => onActiveIdChange(0)}
            className={cn(
              "fluent-indicator flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-sm transition-colors duration-150",
              activeId === 0
                ? "fluent-indicator-active bg-primary-hover-deep text-foreground font-medium"
                : "text-muted-foreground hover:bg-primary-hover hover:text-foreground",
            )}
          >
            <FolderOpen className="size-4" />
            <span>{t("sidebar.allWallpapers")}</span>
          </button>
        </div>
        {builtin && (
          <div className="h-8">
            <button
              type="button"
              onClick={() => onActiveIdChange(builtin.id)}
              className={cn(
                "fluent-indicator flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-sm transition-colors duration-150",
                activeId === builtin.id
                  ? "fluent-indicator-active bg-primary-hover-deep text-foreground font-medium"
                  : "text-muted-foreground hover:bg-primary-hover hover:text-foreground",
              )}
            >
              <Heart className="size-4 fill-[#ef4444] text-[#ef4444]" />
              <span>{t("sidebar.builtinFavorites")}</span>
            </button>
          </div>
        )}
      </div>

      <Separator className="my-1" />

      {/* 中间可滚动：两个手风琴，各自折叠、各自内部拖拽排序 */}
      <div className="flex-1 overflow-y-auto p-1">
        <div className="mb-1 flex items-center justify-between px-3">
          <button
            type="button"
            onClick={toggleNormal}
            className="flex items-center gap-1 text-xs font-medium uppercase tracking-wide text-foreground/40 hover:text-foreground"
          >
            {normalExpanded ? (
              <ChevronDown className="size-3" />
            ) : (
              <ChevronRight className="size-3" />
            )}
            <Star className="size-3" />
            {t("sidebar.collections")}
          </button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="icon"
                className="size-6 text-muted-foreground hover:text-foreground hover:bg-primary-hover"
              >
                <Plus className="size-3" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-40">
              <DropdownMenuItem onClick={openCreateDialog}>
                <Star className="mr-2 size-3.5" />
                {t("sidebar.newManualCollection")}
              </DropdownMenuItem>
              <DropdownMenuItem onClick={openCreateSmart}>
                <Sparkles className="mr-2 size-3.5" />
                {t("sidebar.newSmartCollection")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        {normalExpanded && (
          <Suspense
            fallback={
              <div className="flex h-24 items-center justify-center">
                <div className="size-6 animate-spin rounded-full border-2 border-primary border-t-transparent" />
              </div>
            }
          >
            <SortableCollectionList
              collections={normalCollections}
              activeId={activeId}
              onActiveIdChange={onActiveIdChange}
              onEditSmart={openEditSmart}
              onRename={openRenameDialog}
              onDelete={setDeleteTarget}
            />
          </Suspense>
        )}

        <Separator className="my-1" />

        <div className="mb-1 flex items-center px-3">
          <button
            type="button"
            onClick={toggleWatched}
            className="flex items-center gap-1 text-xs font-medium uppercase tracking-wide text-foreground/40 hover:text-foreground"
          >
            {watchedExpanded ? (
              <ChevronDown className="size-3" />
            ) : (
              <ChevronRight className="size-3" />
            )}
            <FolderSync className="size-3" />
            {t("watched.section")}
          </button>
        </div>

        {watchedExpanded && (
          <Suspense
            fallback={
              <div className="flex h-24 items-center justify-center">
                <div className="size-6 animate-spin rounded-full border-2 border-primary border-t-transparent" />
              </div>
            }
          >
            <SortableCollectionList
              collections={watchedCollections}
              activeId={activeId}
              onActiveIdChange={onActiveIdChange}
              onEditSmart={openEditSmart}
              onRename={openRenameDialog}
              onDelete={setDeleteTarget}
            />
          </Suspense>
        )}
      </div>

      <Separator className="my-1" />

      {/* 回收站置底，不随中间列表滚动 */}
      <div className="shrink-0 p-1">
        <div className="h-8">
          <button
            type="button"
            onClick={() => onActiveIdChange(TRASH_VIEW_ID)}
            className={cn(
              "fluent-indicator flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-sm transition-colors duration-150",
              activeId === TRASH_VIEW_ID
                ? "fluent-indicator-active bg-primary-hover-deep text-foreground font-medium"
                : "text-muted-foreground hover:bg-primary-hover hover:text-foreground",
            )}
          >
            <Trash2 className="size-4" />
            <span>{t("sidebar.trash")}</span>
          </button>
        </div>
      </div>

      {/* 新建/重命名 Dialog */}
      <Dialog open={dialogMode !== null} onOpenChange={() => closeDialog()}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>
              {dialogMode === "create" ? t("sidebar.newCollection") : t("sidebar.renameCollection")}
            </DialogTitle>
          </DialogHeader>
          <div className="py-2">
            <Input
              value={dialogValue}
              onChange={(e) => {
                setDialogValue(e.target.value);
                if (dialogError) setDialogError("");
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleDialogConfirm();
              }}
              placeholder={t("sidebar.enterName")}
              maxLength={32}
              autoFocus
            />
            <div className="mt-1.5 flex items-center justify-between">
              {dialogError ? <p className="text-sm text-destructive">{dialogError}</p> : <span />}
              <span className="text-xs text-muted-foreground">{dialogValue.length}/32</span>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={closeDialog}>
              {t("sidebar.cancel")}
            </Button>
            <Button onClick={handleDialogConfirm}>{t("sidebar.confirm")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 删除确认 Dialog */}
      <AlertDialog open={!!deleteTarget} onOpenChange={() => setDeleteTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("sidebar.deleteConfirmTitle")}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("sidebar.deleteConfirmDesc", { name: deleteTarget?.name })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("sidebar.cancel")}</AlertDialogCancel>
            <AlertDialogAction
              onClick={handleDelete}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              {t("sidebar.delete")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {smartDialogOpen && (
        <Suspense fallback={null}>
          <SmartCollectionDialog
            open={smartDialogOpen}
            onOpenChange={setSmartDialogOpen}
            editing={smartEditing}
          />
        </Suspense>
      )}
    </div>
  );
});

Sidebar.displayName = "Sidebar";

export default Sidebar;
