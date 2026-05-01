import { useCallback, useEffect, useState } from "react";
import { FolderOpen, Pencil, Plus, Star, Trash2 } from "lucide-react";
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
import React from "react";

interface SidebarProps {
  activeId: number;
  onActiveIdChange: (id: number) => void;
}

const Sidebar: React.FC<SidebarProps> = React.memo(({ activeId, onActiveIdChange }) => {
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
      <div className="flex-1 overflow-y-auto px-2 py-2">
        {/* 壁纸库 */}
        <div className="mb-1">
          <button
            type="button"
            onClick={() => onActiveIdChange(0)}
            className={cn(
              "fluent-indicator flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-[13px] transition-all duration-150",
              activeId === 0
                ? "fluent-indicator-active bg-foreground/6 text-foreground font-medium"
                : "text-foreground/65 hover:bg-foreground/4 hover:text-foreground",
            )}
          >
            <FolderOpen className="size-4" />
            <span>{t("sidebar.allWallpapers")}</span>
          </button>
        </div>

        <Separator className="my-2" />

        {/* 收藏夹标题 + 新建按钮 */}
        <div className="mb-1 flex items-center justify-between px-3">
          <span className="text-xs font-medium uppercase tracking-wide text-foreground/40">{t("sidebar.collections")}</span>
          <Button
            variant="ghost"
            size="icon"
            className="size-6 text-foreground/50 hover:text-foreground hover:bg-foreground/5"
            onClick={openCreateDialog}
          >
            <Plus className="size-3" />
          </Button>
        </div>

        {/* 收藏夹列表 */}
        <div className="space-y-0.5">
          {collections.map((collection) => (
            <ContextMenu key={collection.id}>
              <ContextMenuTrigger className="block w-full min-w-0">
                <Tooltip>
                  <TooltipTrigger asChild>
                    <button
                      type="button"
                      onClick={() => onActiveIdChange(collection.id)}
                      className={cn(
                        "fluent-indicator flex w-full min-w-0 items-center gap-2 overflow-hidden rounded-md px-3 py-1.5 text-[13px] transition-all duration-150",
                        activeId === collection.id
                          ? "fluent-indicator-active bg-foreground/6 text-foreground font-medium"
                          : "text-foreground/65 hover:bg-foreground/4 hover:text-foreground",
                      )}
                    >
                      <Star className="size-4 shrink-0" />
                      <span className="block max-w-[120px] truncate">{collection.name}</span>
                    </button>
                  </TooltipTrigger>
                  <TooltipContent side="right">
                    {collection.name}
                  </TooltipContent>
                </Tooltip>
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
          ))}

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