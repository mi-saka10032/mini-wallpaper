import { convertFileSrc } from "@tauri-apps/api/core";
import { ChevronLeft, ChevronRight, Heart, Maximize2, Minimize2, Pencil, X } from "lucide-react";
import { memo, useCallback, useEffect, useMemo, useRef, useState, type FC } from "react";
import { useTranslation } from "react-i18next";

import type { Wallpaper } from "@/api/config";
import { getTags, getWallpaperTags, setWallpaperTags } from "@/api/tag";
import { Button } from "@/components/ui/button";
import { toast } from "@/components/ui/toast";
import TagInput from "@/components/wallpaper/TagInput";
import { cn } from "@/lib/utils";
import { useFavoritesStore } from "@/stores/favoritesStore";
import { usePreviewStore } from "@/stores/previewStore";

/** 胶片条只渲染当前前后各 6 张，避免大库时把缩略图全塞进 DOM */
const FILMSTRIP_RADIUS = 6;

type ZoomMode = "fit" | "actual";

/** 预加载相邻图片（前后各 1 张），减少切换时白屏 */
function usePreloadAdjacent(wallpapers: Wallpaper[], currentIndex: number) {
  useEffect(() => {
    for (const idx of [currentIndex - 1, currentIndex + 1]) {
      if (idx < 0 || idx >= wallpapers.length) continue;
      const wp = wallpapers[idx];
      if (wp.type === "video") continue;
      const img = new Image();
      img.src = convertFileSrc(wp.file_path);
    }
  }, [wallpapers, currentIndex]);
}

/**
 * 全屏预览灯箱（A1 + A2）。
 *
 * ## 当前位置以 id 为锚
 * 不用 `currentIndex` 作为真相：取消收藏 / 改标签 / 删除等写操作会经 pubsub
 * 刷新 viewWallpapers，MainContent 再把派生列表同步到 previewStore。列表一旦
 * 变化，index 就会错位；这里始终用 wallpaperId 定位，index 由 findIndex 派生。
 *
 * ## 锚点消失的重定位（store 驱动，不在点击回调里预判）
 * 写操作发起时只记录「原下一张 id」（最后一张则回退上一张），不跳转、不关闭；
 * 等新列表真正到达后，若锚点已不在列表里才决定是否跳到那张。这样后端失败
 * 回滚时锚点仍在、灯箱原地不动，不会白关一次；同一套逻辑也覆盖改标签后不再
 * 匹配筛选、智能夹规则变更、后台导入重排等所有「当前张消失」的场景。
 */
const PreviewDialog: FC = memo(() => {
  const { t } = useTranslation();
  const wallpapers = usePreviewStore((s) => s.wallpapers);
  const initialIndex = usePreviewStore((s) => s.index);
  const close = usePreviewStore((s) => s.close);
  const favoriteIds = useFavoritesStore((s) => s.favoriteIds);
  const toggleFavorite = useFavoritesStore((s) => s.toggle);

  const [anchorId, setAnchorId] = useState<number | null>(
    () => wallpapers[initialIndex ?? 0]?.id ?? null,
  );
  const [zoomMode, setZoomMode] = useState<ZoomMode>("fit");
  const [tags, setTags] = useState<string[]>([]);
  const [tagDraft, setTagDraft] = useState<string[]>([]);
  const [editingTags, setEditingTags] = useState(false);
  const [suggestions, setSuggestions] = useState<string[]>([]);

  const pendingNextRef = useRef<number | null>(null);
  const closedRef = useRef(false);
  const tagsCacheRef = useRef<Map<number, string[]>>(new Map());
  const dragRef = useRef<{ x: number; y: number; left: number; top: number } | null>(null);

  const currentIndex = useMemo(() => {
    const found = wallpapers.findIndex((w) => w.id === anchorId);
    return found >= 0 ? found : Math.min(initialIndex ?? 0, Math.max(0, wallpapers.length - 1));
  }, [wallpapers, anchorId, initialIndex]);

  const wallpaper = wallpapers[currentIndex];
  const wallpaperId = wallpaper?.id;
  const hasPrev = currentIndex > 0;
  const hasNext = currentIndex < wallpapers.length - 1;

  // ===== 列表变化后的统一重定位 =====
  useEffect(() => {
    if (initialIndex === null || closedRef.current) return;
    if (wallpapers.length === 0) {
      // 空列表：现状会渲染一个空白遮罩只能靠 Esc 退出，这里主动关闭并给反馈。
      closedRef.current = true;
      close();
      toast(t("preview.emptyHint"));
      return;
    }
    if (wallpapers.some((w) => w.id === anchorId)) {
      // 写操作未让当前张消失（含失败回滚），清掉待跳转目标即可。
      pendingNextRef.current = null;
      return;
    }
    const candidates = [
      pendingNextRef.current,
      wallpapers[Math.min(currentIndex, wallpapers.length - 1)]?.id,
      wallpapers[0].id,
    ];
    const next = candidates.find(
      (id): id is number => id != null && wallpapers.some((w) => w.id === id),
    );
    if (next !== undefined) setAnchorId(next);
    pendingNextRef.current = null;
  }, [wallpapers, anchorId, initialIndex, currentIndex, close, t]);

  /** 写操作发起前记录原下一张 id；取消的是最后一张时回退上一张。 */
  const recordPendingNext = useCallback(() => {
    const idx = wallpapers.findIndex((w) => w.id === anchorId);
    if (idx < 0) return;
    pendingNextRef.current = wallpapers[idx + 1]?.id ?? wallpapers[idx - 1]?.id ?? null;
  }, [wallpapers, anchorId]);

  // ===== 导航 =====
  const goTo = useCallback((id: number) => {
    setAnchorId(id);
    setEditingTags(false);
    setZoomMode("fit");
  }, []);

  const goPrev = useCallback(() => {
    if (currentIndex > 0) goTo(wallpapers[currentIndex - 1].id);
  }, [currentIndex, wallpapers, goTo]);

  const goNext = useCallback(() => {
    if (currentIndex < wallpapers.length - 1) goTo(wallpapers[currentIndex + 1].id);
  }, [currentIndex, wallpapers, goTo]);

  const goFirst = useCallback(() => {
    if (wallpapers.length) goTo(wallpapers[0].id);
  }, [wallpapers, goTo]);

  const goLast = useCallback(() => {
    if (wallpapers.length) goTo(wallpapers[wallpapers.length - 1].id);
  }, [wallpapers, goTo]);

  // ===== 键盘（输入框聚焦时不劫持） =====
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (
        target &&
        (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)
      ) {
        if (e.key === "Escape") {
          target.blur();
          setEditingTags(false);
        }
        return;
      }
      switch (e.key) {
        case "Escape":
          close();
          break;
        case "ArrowLeft":
          goPrev();
          break;
        case "ArrowRight":
          goNext();
          break;
        case "Home":
          goFirst();
          break;
        case "End":
          goLast();
          break;
        case "+":
        case "=":
          setZoomMode("actual");
          break;
        case "-":
          setZoomMode("fit");
          break;
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [close, goPrev, goNext, goFirst, goLast]);

  // ===== 标签（按 id 缓存，避免来回翻页重复请求） =====
  useEffect(() => {
    void getTags()
      .then((list) => setSuggestions(list.map((x) => x.name)))
      .catch(() => {});
  }, []);

  useEffect(() => {
    if (wallpaperId === undefined) return;
    const cached = tagsCacheRef.current.get(wallpaperId);
    if (cached) {
      setTags(cached);
      setTagDraft(cached);
      return;
    }
    void getWallpaperTags(wallpaperId)
      .then((list) => {
        const names = list.map((x) => x.name);
        tagsCacheRef.current.set(wallpaperId, names);
        setTags(names);
        setTagDraft(names);
      })
      .catch(() => {});
  }, [wallpaperId]);

  const handleSaveTags = useCallback(async () => {
    if (wallpaperId === undefined) return;
    recordPendingNext();
    try {
      const updated = await setWallpaperTags(wallpaperId, tagDraft);
      const names = updated.map((x) => x.name);
      tagsCacheRef.current.set(wallpaperId, names);
      setTags(names);
      setEditingTags(false);
      toast.success(t("preview.tagsSaved"));
    } catch {
      return;
    }
  }, [wallpaperId, tagDraft, recordPendingNext, t]);

  const handleToggleFavorite = useCallback(async () => {
    if (wallpaperId === undefined) return;
    recordPendingNext();
    try {
      await toggleFavorite(wallpaperId);
    } catch {
      return;
    }
  }, [wallpaperId, toggleFavorite, recordPendingNext]);

  usePreloadAdjacent(wallpapers, currentIndex);

  if (!wallpaper) return null;

  const src = convertFileSrc(wallpaper.file_path);
  const fileSize = wallpaper.file_size
    ? `${(wallpaper.file_size / 1024 / 1024).toFixed(1)} MB`
    : "";
  const resolution =
    wallpaper.width && wallpaper.height ? `${wallpaper.width} × ${wallpaper.height}` : "";
  const isFavorite = favoriteIds.has(wallpaper.id);
  const isVideo = wallpaper.type === "video";
  const actualZoom = zoomMode === "actual" && !isVideo;

  const stripStart = Math.max(0, currentIndex - FILMSTRIP_RADIUS);
  const strip = wallpapers.slice(stripStart, currentIndex + FILMSTRIP_RADIUS + 1);

  const stop = (e: React.MouseEvent) => e.stopPropagation();

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center rounded-xl bg-black/70 px-16 pb-44 backdrop-blur-xl"
      onClick={close}
    >
      {/* 顶部：缩放切换 + 关闭 */}
      <div className="fixed right-6 top-6 z-50 flex items-center gap-2">
        {!isVideo && (
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              setZoomMode(zoomMode === "fit" ? "actual" : "fit");
            }}
            title={actualZoom ? t("preview.fit") : t("preview.actual")}
            className="rounded-full bg-white/8 p-2 text-white/60 transition-colors hover:bg-white/15 hover:text-white"
          >
            {actualZoom ? <Minimize2 className="size-5" /> : <Maximize2 className="size-5" />}
          </button>
        )}
        <button
          type="button"
          onClick={close}
          title={t("preview.close")}
          className="rounded-full bg-white/8 p-2 text-white/60 transition-colors hover:bg-white/15 hover:text-white"
        >
          <X className="size-5" />
        </button>
      </div>

      {/* 左右翻页 */}
      {hasPrev && (
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            goPrev();
          }}
          className="fixed left-6 top-1/2 z-50 -translate-y-1/2 rounded-full bg-white/8 p-2.5 text-white/60 transition-colors hover:bg-white/15 hover:text-white"
        >
          <ChevronLeft className="size-6" />
        </button>
      )}
      {hasNext && (
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            goNext();
          }}
          className="fixed right-6 top-1/2 z-50 -translate-y-1/2 rounded-full bg-white/8 p-2.5 text-white/60 transition-colors hover:bg-white/15 hover:text-white"
        >
          <ChevronRight className="size-6" />
        </button>
      )}

      {/* 主内容区：1:1 时用滚动容器 + 拖拽平移 */}
      <div
        className={cn(
          "flex max-h-[68vh] max-w-[80vw] items-center justify-center",
          actualZoom && "overflow-auto",
        )}
        onClick={stop}
        onPointerDown={(e) => {
          if (!actualZoom) return;
          e.stopPropagation();
          dragRef.current = {
            x: e.clientX,
            y: e.clientY,
            left: e.currentTarget.scrollLeft,
            top: e.currentTarget.scrollTop,
          };
          e.currentTarget.setPointerCapture(e.pointerId);
        }}
        onPointerMove={(e) => {
          const drag = dragRef.current;
          if (!drag) return;
          e.currentTarget.scrollLeft = drag.left - (e.clientX - drag.x);
          e.currentTarget.scrollTop = drag.top - (e.clientY - drag.y);
        }}
        onPointerUp={() => {
          dragRef.current = null;
        }}
      >
        {isVideo ? (
          <video
            key={wallpaper.id}
            src={src}
            controls
            autoPlay
            muted
            className="max-h-[68vh] max-w-[80vw] rounded-md"
          />
        ) : (
          <img
            key={wallpaper.id}
            src={src}
            alt={wallpaper.name}
            draggable={false}
            className={cn(
              "rounded-md object-contain",
              actualZoom ? "max-w-none" : "max-h-[68vh] max-w-[80vw]",
            )}
          />
        )}
      </div>

      {/* 底部统一面板：上为胶片条，下为信息与就地操作 */}
      <div
        className="fixed bottom-4 left-1/2 z-50 w-[min(92vw,900px)] -translate-x-1/2 overflow-hidden rounded-xl bg-black/60 text-white/70 backdrop-blur-xl"
        onClick={stop}
      >
        <div className="flex gap-1.5 overflow-x-auto px-3 py-2">
          {strip.map((wp) => (
            <button
              key={wp.id}
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                goTo(wp.id);
              }}
              title={wp.name}
              className={cn(
                "size-12 shrink-0 overflow-hidden rounded border border-white/10 transition-opacity",
                wp.id === wallpaper.id ? "ring-2 ring-primary" : "opacity-60 hover:opacity-100",
              )}
            >
              <img
                src={convertFileSrc(wp.thumb_path ?? wp.file_path)}
                alt=""
                draggable={false}
                className="size-full object-cover"
              />
            </button>
          ))}
        </div>

        <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 border-t border-white/10 px-4 py-2.5 text-[11px]">
          <span className="max-w-40 truncate font-medium text-white/90">{wallpaper.name}</span>
          {resolution && <span>{resolution}</span>}
          {fileSize && <span>{fileSize}</span>}
          <span className="uppercase">{t(`preview.${wallpaper.type}`)}</span>
          {wallpaper.duration != null && (
            <span>
              {t("preview.duration")} {wallpaper.duration}s
            </span>
          )}
          <span>
            {t("preview.createdAt")} {wallpaper.created_at.slice(0, 16)}
          </span>
          <span>
            {t("preview.playCount")} {wallpaper.play_count}
          </span>
          <span className="text-white/40">
            {currentIndex + 1} / {wallpapers.length}
          </span>

          <div className="ml-auto flex items-center gap-1.5">
            {editingTags ? (
              <>
                <TagInput
                  value={tagDraft}
                  onChange={setTagDraft}
                  suggestions={suggestions}
                  className="w-40"
                />
                <Button size="xs" onClick={() => void handleSaveTags()}>
                  {t("preview.save")}
                </Button>
                <Button
                  variant="ghost"
                  size="xs"
                  onClick={() => {
                    setTagDraft(tags);
                    setEditingTags(false);
                  }}
                >
                  {t("preview.cancel")}
                </Button>
              </>
            ) : (
              <Button
                variant="ghost"
                size="xs"
                title={t("preview.editTags")}
                onClick={() => {
                  setTagDraft(tags);
                  setEditingTags(true);
                }}
              >
                <Pencil className="size-3" />
                <span className="max-w-32 truncate">
                  {tags.length ? tags.join(", ") : t("preview.tags")}
                </span>
              </Button>
            )}
            <Button
              variant="ghost"
              size="icon-xs"
              onClick={() => void handleToggleFavorite()}
              title={isFavorite ? t("preview.unfavorite") : t("preview.favorite")}
            >
              <Heart className={cn("size-3.5", isFavorite && "fill-red-500 text-red-500")} />
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
});

PreviewDialog.displayName = "PreviewDialog";

export default PreviewDialog;
