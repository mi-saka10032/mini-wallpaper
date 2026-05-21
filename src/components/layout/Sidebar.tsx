import { useCallback, useEffect, useState } from "react";
import { FolderOpen, Heart, Pencil, Plus, Star, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Separator } from "@/components/ui/separator";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
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
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useCollectionStore, type Collection } from "@/stores/collectionStore";
import { memo, type FC } from "react";

interface SidebarProps {
  activeId: number;
  onActiveIdChange: (id: number) => void;
}

const Sidebar: FC<SidebarProps> = memo(({ activeId, onActiveIdChange }) => {
  const { t } = useTranslation();
  const collections = useCollectionStore((s) => s.collections);
  const fetchCollections = useCollectionStore((s) => s.fetchCollections);
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

  useEffect(() => {
    fetchCollections();
  }, [fetchCollections]);

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
    <div className="flex h-full w-52 shrink-0 flex-col overflow-hidden border-r border-border/50 bg-sidebar-background">
      <div className="flex-1 overflow-y-auto p-1">
        {/* 壁纸库 */}
        <div className="h-8">
          <button
            type="button"
            onClick={() => onActiveIdChange(0)}
            className={cn(
              "fluent-indicator flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-[13px] transition-all duration-150",
              activeId === 0
                ? "fluent-indicator-active bg-primary-hover-deep text-foreground font-medium"
                : "text-foreground/65 hover:bg-primary-hover hover:text-foreground",
            )}
          >
            <FolderOpen className="size-4" />
            <span>{t("sidebar.allWallpapers")}</span>
          </button>
        </div>

        <Separator className="my-1" />

        {/* 收藏夹标题 + 新建按钮 */}
        <div className="mb-1 flex items-center justify-between px-3">
          <span className="text-xs font-medium uppercase tracking-wide text-foreground/40">{t("sidebar.collections")}</span>
          <Button
            variant="ghost"
            size="icon"
            className="size-6 text-foreground/50 hover:text-foreground hover:bg-primary-hover"
            onClick={openCreateDialog}
          >
            <Plus className="size-3" />
          </Button>
        </div>

        {/* 收藏夹列表 */}
        <div className="space-y-0.5">
          {collections.map((collection) => {
            const isBuiltin = collection.is_builtin === 1;
            // 内置收藏夹的展示名走 i18n（让中英文环境分别显示「我喜欢」/ "My Favorites"），
            // 用户自建收藏夹直接显示 DB 中的名字
            const displayName = isBuiltin ? t("sidebar.builtinFavorites") : collection.name;
            const Icon = isBuiltin ? Heart : Star;

            const trigger = (
              <Tooltip>
                <TooltipTrigger asChild>
                  <button
                    type="button"
                    onClick={() => onActiveIdChange(collection.id)}
                    className={cn(
                      "fluent-indicator flex w-full min-w-0 items-center gap-2 overflow-hidden rounded-md px-3 py-1.5 text-[13px] transition-all duration-150",
                      activeId === collection.id
                        ? "fluent-indicator-active bg-primary-hover-deep text-foreground font-medium"
                        : "text-foreground/65 hover:bg-primary-hover hover:text-foreground",
                    )}
                  >
                    <Icon
                      className={cn(
                        "size-4 shrink-0",
                        // 内置收藏夹用主题红色填充心形，强化"系统级"视觉
                        isBuiltin && "fill-[#ef4444] text-[#ef4444]",
                      )}
                    />
                    <span className="block max-w-[120px] truncate">{displayName}</span>
                  </button>
                </TooltipTrigger>
                <TooltipContent side="right">{displayName}</TooltipContent>
              </Tooltip>
            );

            // 内置收藏夹：不挂右键菜单（用户无法重命名/删除）
            if (isBuiltin) {
              return (
                <div key={collection.id} className="block w-full min-w-0">
                  {trigger}
                </div>
              );
            }

            // 用户自建收藏夹：挂载完整的右键菜单
            return (
              <ContextMenu key={collection.id}>
                <ContextMenuTrigger className="block w-full min-w-0">
                  {trigger}
                </ContextMenuTrigger>
                <ContextMenuContent className="w-32">
                  <ContextMenuItem onClick={() => openRenameDialog(collection)}>
                    <Pencil className="mr-2 size-3.5" />
                    {t("sidebar.rename")}
                  </ContextMenuItem>
                  <ContextMenuItem
                    onClick={() => setDeleteTarget(collection)}
                    className="text-destructive focus:text-destructive"
                  >
                    <Trash2 className="mr-2 size-3.5" />
                    {t("sidebar.delete")}
                  </ContextMenuItem>
                </ContextMenuContent>
              </ContextMenu>
            );
          })}

          {collections.length === 0 && (
            <p className="px-3 py-2 text-xs text-foreground/35">{t("sidebar.noCollections")}</p>
          )}
        </div>
      </div>



      {/* 新建/重命名 Dialog */}
      <Dialog open={dialogMode !== null} onOpenChange={() => closeDialog()}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>{dialogMode === "create" ? t("sidebar.newCollection") : t("sidebar.renameCollection")}</DialogTitle>
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
              <span className="text-xs text-foreground/50">{dialogValue.length}/32</span>
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
    </div>
  );
});

Sidebar.displayName = "Sidebar";

export default Sidebar;