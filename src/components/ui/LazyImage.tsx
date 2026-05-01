import { memo, useCallback, useEffect, useState } from "react";
import { ImageOff } from "lucide-react";
import { cn } from "@/lib/utils";

interface LazyImageProps {
  src: string;
  alt: string;
  className?: string;
  /** 加载失败时的自定义 fallback 内容 */
  fallback?: React.ReactNode;
}

/**
 * LazyImage - 带骨架屏、淡入过渡和加载失败兜底的图片组件
 *
 * 特性：
 * - 加载中：shimmer 骨架动画
 * - 加载完成：fade-in 过渡
 * - 加载失败：友好的 fallback UI
 */
const LazyImage: React.FC<LazyImageProps> = ({ src, alt, className, fallback }) => {
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState(false);

  // src 变化时重置加载状态，避免旧状态残留
  useEffect(() => {
    setLoaded(false);
    setError(false);
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
        src={src}
        alt={alt}
        loading="lazy"
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