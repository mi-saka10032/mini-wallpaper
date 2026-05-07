import { memo, useCallback, useEffect, useRef, useState, type FC, type ReactNode } from "react";
import { ImageOff } from "lucide-react";
import { cn } from "@/lib/utils";

interface LazyImageProps {
  src: string;
  alt: string;
  className?: string;
  /** 加载失败时的自定义 fallback 内容 */
  fallback?: ReactNode;
}

/**
 * LazyImage - 带骨架屏、淡入过渡和加载失败兜底的图片组件
 *
 * 特性：
 * - 加载中：shimmer 骨架动画
 * - 加载完成：fade-in 过渡
 * - 加载失败：友好的 fallback UI
 * - 修复：src 变化时正确处理缓存命中场景，避免 onLoad 与 useEffect 的竞态条件
 */
const LazyImage: FC<LazyImageProps> = ({ src, alt, className, fallback }) => {
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState(false);
  const imgRef = useRef<HTMLImageElement>(null);
  const srcRef = useRef(src);

  // src 变化时重置加载状态
  if (srcRef.current !== src) {
    srcRef.current = src;
    // 同步重置，避免 useEffect 异步执行导致的竞态
    if (loaded) setLoaded(false);
    if (error) setError(false);
  }

  // 在 DOM 更新后检查图片是否已经加载完成（缓存命中场景）
  // useEffect 在 commit 后执行，此时 img.src 已经更新
  useEffect(() => {
    const img = imgRef.current;
    if (img && img.complete && img.naturalWidth > 0 && img.src) {
      setLoaded(true);
    }
  }, [src]);

  const handleLoad = useCallback(() => {
    setLoaded(true);
  }, []);

  const handleError = useCallback(() => {
    setError(true);
  }, []);

  if (error) {
    return (
      <div className={cn("flex size-full items-center justify-center bg-foreground/4", className)}>
        {fallback ?? (
          <div className="flex flex-col items-center gap-1 text-foreground/30">
            <ImageOff className="size-6" strokeWidth={1.5} />
          </div>
        )}
      </div>
    );
  }

  return (
    <div className={cn("relative size-full overflow-hidden bg-foreground/4", className)}>
      {/* 骨架屏 shimmer 动画 */}
      {!loaded && (
        <div className="absolute inset-0 animate-pulse bg-gradient-to-r from-foreground/3 via-foreground/5 to-foreground/3" />
      )}
      {/* 实际图片 */}
      <img
        ref={imgRef}
        src={src}
        alt={alt}
        onLoad={handleLoad}
        onError={handleError}
        className={cn(
          "size-full object-cover transition-opacity duration-300",
          loaded ? "opacity-100" : "opacity-0",
        )}
      />
    </div>
  );
};

export default memo(LazyImage);
