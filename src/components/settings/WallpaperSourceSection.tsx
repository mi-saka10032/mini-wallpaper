import React from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { AlertTriangle, FolderOpen, Image } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { Wallpaper } from "@/api/config";

interface Collection {
  id: number;
  name: string;
}

interface WallpaperSourceSectionProps {
  sourceType: "wallpaper" | "collection";
  wallpapers: Wallpaper[];
  collections: Collection[];
  selectedWallpaperId: number | null;
  selectedCollectionId: number | null;
  collectionWarning: string | null;
  onSourceChange: (source: "wallpaper" | "collection") => void;
  onWallpaperSelect: (wallpaperId: number) => void;
  onCollectionSelect: (collectionIdStr: string) => void;
}

/** 壁纸来源选择区块 */
const WallpaperSourceSection: React.FC<WallpaperSourceSectionProps> = React.memo(({
  sourceType,
  wallpapers,
  collections,
  selectedWallpaperId,
  selectedCollectionId,
  collectionWarning,
  onSourceChange,
  onWallpaperSelect,
  onCollectionSelect,
}) => {
  const { t } = useTranslation();

  return (
    <div className="space-y-3">
      <Label className="text-sm font-medium">{t("monitor.wallpaperSource")}</Label>
      <div className="flex gap-2">
        <Button
          variant={sourceType === "wallpaper" ? "default" : "outline"}
          size="sm"
          onClick={() => onSourceChange("wallpaper")}
        >
          <Image className="mr-1.5 size-3.5" />
          {t("monitor.singleWallpaper")}
        </Button>
        <Button
          variant={sourceType === "collection" ? "default" : "outline"}
          size="sm"
          onClick={() => onSourceChange("collection")}
        >
          <FolderOpen className="mr-1.5 size-3.5" />
          {t("monitor.collectionRotation")}
        </Button>
      </div>

      {/* 单张壁纸选择 */}
      {sourceType === "wallpaper" && (
        <div className="space-y-2">
          <Select
            value={selectedWallpaperId?.toString() ?? ""}
            onValueChange={(v) => onWallpaperSelect(Number(v))}
          >
            <SelectTrigger className="w-full">
              <SelectValue placeholder={t("monitor.selectWallpaper")} />
            </SelectTrigger>
            <SelectContent>
              {wallpapers.map((wp) => (
                <SelectItem key={wp.id} value={wp.id.toString()}>
                  <div className="flex items-center gap-2">
                    {wp.thumb_path ? (
                      <img
                        src={convertFileSrc(wp.thumb_path)}
                        alt=""
                        className="size-6 rounded object-cover"
                      />
                    ) : (
                      <div className="flex size-6 items-center justify-center rounded bg-muted">
                        <Image className="size-3 text-muted-foreground" />
                      </div>
                    )}
                    <span className="truncate text-sm">{wp.name}</span>
                  </div>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )}

      {/* 收藏夹选择 */}
      {sourceType === "collection" && (
        <div className="space-y-2">
          {collections.length === 0 ? (
            <p className="rounded-md border border-dashed border-border p-3 text-center text-sm text-muted-foreground">
              {t("monitor.noCollectionHint")}
            </p>
          ) : (
            <>
              <Select
                value={selectedCollectionId?.toString() ?? ""}
                onValueChange={onCollectionSelect}
              >
                <SelectTrigger className="w-full">
                  <SelectValue placeholder={t("monitor.selectCollection")} />
                </SelectTrigger>
                <SelectContent>
                  {collections.map((col) => (
                    <SelectItem key={col.id} value={col.id.toString()}>
                      {col.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {collectionWarning && (
                <div className="flex items-center gap-1.5 rounded-md border border-yellow-500/50 bg-yellow-500/10 px-3 py-2 text-xs text-yellow-600 dark:text-yellow-400">
                  <AlertTriangle className="size-3.5 shrink-0" />
                  {collectionWarning}
                </div>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
});

WallpaperSourceSection.displayName = "WallpaperSourceSection";

export default WallpaperSourceSection;
