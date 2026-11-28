import { convertFileSrc } from "@tauri-apps/api/core";
import { ChevronLeft, ChevronRight, Heart, Maximize2, Minimize2, Pencil, X } from "lucide-react";
import {
  lazy,
  memo,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type FC,
} from "react";
import { useTranslation } from "react-i18next";

import type { Wallpaper } from "@/api/config";
import { getWallpaperTags } from "@/api/tag";
import { getById } from "@/api/wallpaper";
import { Button } from "@/components/ui/button";
import { toast } from "@/components/ui/toast";
import { cn } from "@/lib/utils";
import { useFavoritesStore } from "@/stores/favoritesStore";
import { usePreviewStore } from "@/stores/previewStore";

// 标签编辑复用网格右键菜单同款弹窗，只在点开时加载。
const TagEditorDialog = lazy(() => import("@/components/wallpaper/TagEditorDialog"));

/** 胶片条只渲染当前前后各 6 张，避免大库时把缩略图全塞进 DOM */
const FILMSTRIP_RADIUS = 6;

type ZoomMode = "fit" | "actual";

/** 预加载相邻图片（前后各 1 张），减少切换时白屏 */
function usePreloadAdjacent(wallpapers: Wallpaper[], currentIndex: number) {
  useEffect(() => {
    const len = wallpapers.length;
    if (len === 0) return;
    // 环形：末尾的下一张是首张，首张的上一张是末尾。
    const indices = new Set([(currentIndex - 1 + len) % len, (currentIndex + 1) % len]);
    for (const idx of indices) {
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
  const [tagDialogOpen, setTagDialogOpen] = useState(false);
  /** 按需拉取的单条 play_count；null 表示还没拿到或拉取失败，回退到缓存值 */
  const [freshPlayCount, setFreshPlayCount] = useState<number | null>(null);
  /** 监听壁纸的源文件已被外部删除时，主图/视频加载失败，提示刷新 */
  const [sourceInvalid, setSourceInvalid] = useState(false);

  const pendingNextRef = useRef<number | null>(null);
  const closedRef = useRef(false);
  /** 弹窗关闭的那一次事件循环里，抑制灯箱响应同一个 Esc */
  const suppressEscapeRef = useRef(false);
  const tagsCacheRef = useRef<Map<number, string[]>>(new Map());
  const dragRef = useRef<{ x: number; y: number; left: number; top: number } | null>(null);

  const currentIndex = useMemo(() => {
    const found = wallpapers.findIndex((w) => w.id === anchorId);
    return found >= 0 ? found : Math.min(initialIndex ?? 0, Math.max(0, wallpapers.length - 1));
  }, [wallpapers, anchorId, initialIndex]);

  const wallpaper = wallpapers[currentIndex];
  const wallpaperId = wallpaper?.id;
  // 环形导航：只要列表多于一张，前后都可达。
  const canCycle = wallpapers.length > 1;

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
    setZoomMode("fit");
  }, []);

  const goPrev = useCallback(() => {
    if (!canCycle) return;
    const len = wallpapers.length;
    goTo(wallpapers[(currentIndex - 1 + len) % len].id);
  }, [canCycle, currentIndex, wallpapers, goTo]);

  const goNext = useCallback(() => {
    if (!canCycle) return;
    const len = wallpapers.length;
    goTo(wallpapers[(currentIndex + 1) % len].id);
  }, [canCycle, currentIndex, wallpapers, goTo]);

  const goFirst = useCallback(() => {
    if (wallpapers.length) goTo(wallpapers[0].id);
  }, [wallpapers, goTo]);

  const goLast = useCallback(() => {
    if (wallpapers.length) goTo(wallpapers[wallpapers.length - 1].id);
  }, [wallpapers, goTo]);

  // ===== 键盘（输入框聚焦时不劫持） =====
  useEffect(() => {
    // 标签弹窗打开时不注册快捷键：Esc 该由弹窗消费，方向键也不该翻页。
    if (tagDialogOpen) return;
    const onKeyDown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (
        target &&
        (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)
      ) {
        return;
      }
      switch (e.key) {
        case "Escape":
          // Radix 关闭弹窗会让上面的 effect 同步重注册本监听，同一个 Esc
          // 仍会冒泡到 window；此时 tagDialogOpen 已是 false，必须靠标记拦住。
          if (suppressEscapeRef.current) return;
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
  }, [close, goPrev, goNext, goFirst, goLast, tagDialogOpen]);

  // ===== 标签（按 id 缓存，避免来回翻页重复请求） =====
  useEffect(() => {
    if (wallpaperId === undefined) return;
    const cached = tagsCacheRef.current.get(wallpaperId);
    if (cached) {
      setTags(cached);
      return;
    }
    void getWallpaperTags(wallpaperId)
      .then((list) => {
        const names = list.map((x) => x.name);
        tagsCacheRef.current.set(wallpaperId, names);
        setTags(names);
      })
      .catch(() => {});
  }, [wallpaperId]);

  // 打开灯箱 / 切到另一张时拉一次单条详情，覆盖 play_count。
  // 后端的计数是副作用、不走前端订阅，缓存里可能是旧值；按需单条拉取代价
  // 只有 1 次 IPC，灯箱开着期间不实时刷新。cancelled 防止快速翻页时旧响应覆盖。
  useEffect(() => {
    if (wallpaperId === undefined) return;
    let cancelled = false;
    setFreshPlayCount(null);
    setSourceInvalid(false);
    void getById(wallpaperId)
      .then((wp) => {
        if (!cancelled) setFreshPlayCount(wp?.play_count ?? null);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [wallpaperId]);

  /** 标签弹窗关闭后重取一次，覆盖缓存让 chips 立刻反映保存结果。 */
  const reloadTags = useCallback(() => {
    if (wallpaperId === undefined) return;
    void getWallpaperTags(wallpaperId)
      .then((list) => {
        const names = list.map((x) => x.name);
        tagsCacheRef.current.set(wallpaperId, names);
        setTags(names);
      })
      .catch(() => {});
  }, [wallpaperId]);

  const tagTargetIds = useMemo(
    () => (wallpaperId === undefined ? [] : [wallpaperId]),
    [wallpaperId],
  );

  /**
   * 标签弹窗经 Radix Portal 挂到 body，DOM 上不是遮罩的子节点，
   * 但 React 事件仍沿组件树冒泡到这里。弹窗打开时若照常 close()，
   * 弹窗内任意点击（含点空白处让输入框失焦）都会把底层灯箱一起卸载。
   */
  const handleOverlayClick = useCallback(() => {
    if (!tagDialogOpen) close();
  }, [close, tagDialogOpen]);

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

  // 环形胶片条：接近队尾时把 0 索引处的图拼在后面。
  // 窗口大小取 min(len, 2R+1)，保证同一张图在一个窗口里只出现一次。
  const stripCount = Math.min(wallpapers.length, FILMSTRIP_RADIUS * 2 + 1);
  const half = Math.floor(stripCount / 2);
  const stripStart =
    (((currentIndex - half) % wallpapers.length) + wallpapers.length) % wallpapers.length;
  const strip = Array.from({ length: stripCount }, (_, slot) => ({
    slot,
    wp: wallpapers[(stripStart + slot) % wallpapers.length],
  }));

  const stop = (e: React.MouseEvent) => e.stopPropagation();

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center overflow-hidden rounded-xl bg-black/70 px-16 pb-44"
      onClick={handleOverlayClick}
    >
      {/* 模糊层单独铺成矩形：backdrop-filter 直接作用在带圆角的元素上时，
          WebView2 会在圆角边缘采到窗口外的亮背景，漏出一丝弧形白边
          （顶部被 Toolbar 盖住，所以只有下方两角肉眼可见）。
          改由外层 overflow-hidden + rounded-xl 统一裁剪。 */}
      <div className="pointer-events-none absolute inset-0 backdrop-blur-xl" />

      {/* 顶部：缩放切换 + 关闭 */}
      <div className="absolute right-6 top-6 z-50 flex items-center gap-2">
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
      {canCycle && (
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            goPrev();
          }}
          className="absolute left-6 top-1/2 z-50 -translate-y-1/2 rounded-full bg-white/8 p-2.5 text-white/60 transition-colors hover:bg-white/15 hover:text-white"
        >
          <ChevronLeft className="size-6" />
        </button>
      )}
      {canCycle && (
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            goNext();
          }}
          className="absolute right-6 top-1/2 z-50 -translate-y-1/2 rounded-full bg-white/8 p-2.5 text-white/60 transition-colors hover:bg-white/15 hover:text-white"
        >
          <ChevronRight className="size-6" />
        </button>
      )}

      {/* 主内容区：1:1 时用滚动容器 + 拖拽平移 */}
      <div
        className={cn(
          // 显式提到模糊层之上：absolute 的 backdrop 层是 z-index:auto 的
          // positioned 元素，会绘制在非定位内容之上，否则图片会被一起采样模糊。
          "relative z-10 flex max-h-[68vh] max-w-[80vw] items-center justify-center",
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
            onError={() => setSourceInvalid(true)}
            className="max-h-[68vh] max-w-[80vw] rounded-md"
          />
        ) : (
          <img
            key={wallpaper.id}
            src={src}
            alt={wallpaper.name}
            draggable={false}
            onError={() => setSourceInvalid(true)}
            className={cn(
              "rounded-md object-contain",
              actualZoom ? "max-w-none" : "max-h-[68vh] max-w-[80vw]",
            )}
          />
        )}
        {/* 监听夹内源文件被外部删掉时，缩略图/DB 信息仍在，点开画廊才报错 */}
        {sourceInvalid && (
          <div className="absolute inset-0 flex items-center justify-center rounded-md bg-black/70 text-sm text-white/80">
            {t("watched.invalidPreview")}
          </div>
        )}
      </div>

      {/* 底部统一面板：上为胶片条，下为信息与就地操作 */}
      <div
        className="absolute bottom-4 left-1/2 z-50 w-[min(92vw,900px)] -translate-x-1/2 overflow-hidden rounded-xl bg-black/60 text-white/70 backdrop-blur-xl"
        onClick={stop}
      >
        <div className="flex gap-1.5 overflow-x-auto px-3 py-2">
          {strip.map(({ slot, wp }) => (
            <button
              key={slot}
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
              {/* 无缩略图的视频不能让 <img> 去加载视频文件，会显示碎图 */}
              {!wp.thumb_path && wp.type === "video" ? (
                <div className="flex size-full items-center justify-center bg-white/5 text-[10px] text-white/50">
                  VIDEO
                </div>
              ) : (
                <img
                  src={convertFileSrc(wp.thumb_path ?? wp.file_path)}
                  alt=""
                  draggable={false}
                  className="size-full object-cover"
                />
              )}
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
            {t("preview.playCount")} {freshPlayCount ?? wallpaper.play_count}
          </span>
          <span className="text-white/40">
            {currentIndex + 1} / {wallpapers.length}
          </span>

          <div className="ml-auto flex items-center gap-1.5">
            <Button
              variant="ghost"
              size="xs"
              title={t("preview.editTags")}
              onClick={() => {
                // 保存标签可能让当前张不再匹配筛选视图，先记下原下一张 id。
                recordPendingNext();
                setTagDialogOpen(true);
              }}
            >
              <Pencil className="size-3" />
              <span className="max-w-32 truncate">
                {tags.length ? tags.join(", ") : t("preview.tags")}
              </span>
            </Button>
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

      {/* 标签弹窗由 Radix Portal 挂到 body 末尾，DOM 顺序在预览遮罩之后；
          DialogContent 另设 z-[60]，确保盖住 z-50 的预览灯箱。 */}
      {tagDialogOpen && wallpaperId !== undefined && (
        // contents 不产生布局盒；Portal 的 React 事件先在这里截断，
        // 弹窗内部点击（含 Overlay）一律不冒泡到灯箱遮罩。
        <div className="contents" onClick={stop}>
          <Suspense fallback={null}>
            <TagEditorDialog
              open={tagDialogOpen}
              onOpenChange={(open) => {
                setTagDialogOpen(open);
                if (!open) {
                  reloadTags();
                  // 弹窗关闭后用一个宏任务窗口抑制 Esc：事件冒泡是同步的，
                  // setTimeout(0) 一定在同一次事件派发结束后才清除标记。
                  suppressEscapeRef.current = true;
                  setTimeout(() => {
                    suppressEscapeRef.current = false;
                  }, 0);
                }
              }}
              mode="single"
              wallpaperIds={tagTargetIds}
            />
          </Suspense>
        </div>
      )}
    </div>
  );
});

PreviewDialog.displayName = "PreviewDialog";

export default PreviewDialog;
