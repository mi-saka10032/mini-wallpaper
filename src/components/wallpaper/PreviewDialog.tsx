import { convertFileSrc } from "@tauri-apps/api/core";
import { ChevronLeft, ChevronRight, X } from "lucide-react";
import { memo, useCallback, useEffect, useState, type FC } from "react";
import type { Wallpaper } from "@/api/config";

interface PreviewDialogProps {
  wallpapers: Wallpaper[];
  initialIndex: number;
  onClose: () => void;
}

/** 预加载相邻图片（前后各 1 张），减少切换时白屏 */
function usePreloadAdjacent(wallpapers: Wallpaper[], currentIndex: number) {
  useEffect(() => {
    const indices = [currentIndex - 1, currentIndex + 1];
    for (const idx of indices) {
      if (idx < 0 || idx >= wallpapers.length) continue;
      const wp = wallpapers[idx];
      if (wp.type === "video") continue; // 视频不预加载
      const img = new Image();
      img.src = convertFileSrc(wp.file_path);
    }
  }, [wallpapers, currentIndex]);
}

const PreviewDialog: FC<PreviewDialogProps> = memo(({ wallpapers, initialIndex, onClose }) => {
  const [currentIndex, setCurrentIndex] = useState(initialIndex);

  const wallpaper = wallpapers[currentIndex];
  const hasPrev = currentIndex > 0;
  const hasNext = currentIndex < wallpapers.length - 1;

  const goPrev = useCallback(() => {
    if (hasPrev) setCurrentIndex((i) => i - 1);
  }, [hasPrev]);

  const goNext = useCallback(() => {
    if (hasNext) setCurrentIndex((i) => i + 1);
  }, [hasNext]);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      if (e.key === "ArrowLeft") goPrev();
      if (e.key === "ArrowRight") goNext();
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose, goPrev, goNext]);

  useEffect(() => {
    // 确保 initialIndex 不越界
    const safeIndex = Math.max(0, Math.min(initialIndex, wallpapers.length - 1));
    setCurrentIndex(safeIndex);
  }, [initialIndex, wallpapers.length]);

  // 预加载相邻图片
  usePreloadAdjacent(wallpapers, currentIndex);

  if (!wallpaper) return null;

  const src = convertFileSrc(wallpaper.file_path);
  const fileSize = wallpaper.file_size
    ? `${(wallpaper.file_size / 1024 / 1024).toFixed(1)} MB`
    : "";
  const resolution =
    wallpaper.width && wallpaper.height ? `${wallpaper.width} × ${wallpaper.height}` : "";

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-xl"
      onClick={onClose}
    >
      {/* 关闭按钮 — 固定右上角 */}
      <button
        type="button"
        onClick={onClose}
        className="fixed right-6 top-6 z-50 rounded-full bg-white/8 p-2 text-white/60 transition-colors hover:bg-white/15 hover:text-white"
      >
        <X className="size-5" />
      </button>

      {/* 左导航 — 固定左侧居中 */}
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

      {/* 右导航 — 固定右侧居中 */}
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

      {/* 主内容区 — 居中展示 */}
      <div className="flex flex-col items-center px-16" onClick={(e) => e.stopPropagation()}>
        {wallpaper.type === "video" ? (
          <video
            key={wallpaper.id}
            src={src}
            controls
            autoPlay
            muted
            className="max-h-[80vh] max-w-[80vw] rounded-md"
          />
        ) : (
          <img
            src={src}
            alt={wallpaper.name}
            className="max-h-[80vh] max-w-[80vw] rounded-md object-contain"
          />
        )}
      </div>

      {/* 底部信息 — 固定底部居中 */}
      <div className="fixed bottom-6 left-1/2 z-50 -translate-x-1/2 flex items-center gap-4 rounded-lg bg-black/50 px-5 py-2.5 text-sm text-white/70 backdrop-blur-xl">
        <span className="max-w-48 truncate">{wallpaper.name}</span>
        {resolution && <span className="text-white/50">{resolution}</span>}
        {fileSize && <span className="text-white/50">{fileSize}</span>}
        <span className="uppercase text-white/50">{wallpaper.type}</span>
        <span className="text-white/35">
          {currentIndex + 1} / {wallpapers.length}
        </span>
      </div>
    </div>
  );
});

PreviewDialog.displayName = "PreviewDialog";

export default PreviewDialog;