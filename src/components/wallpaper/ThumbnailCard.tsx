import { convertFileSrc } from "@tauri-apps/api/core";
import { Film, Image } from "lucide-react";
import { memo } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import LazyImage from "@/components/ui/LazyImage";
import type { Wallpaper } from "@/api/config";

// ============ 类型定义 ============

export interface ThumbnailCardProps {
  /** 壁纸数据 */
  wallpaper: Wallpaper;
  /** 点击回调 */
  onClick?: (e: React.MouseEvent) => void;
  /** 右键菜单回调 */
  onContextMenu?: (e: React.MouseEvent) => void;
  /** 外层容器额外 className（用于控制边框、ring 等状态样式） */
  className?: string;
  /** 外层容器 style */
  style?: React.CSSProperties;
  /** 左上角叠加层（如选中指示器、已添加标签等） */
  overlayTopLeft?: React.ReactNode;
  /** 右下角叠加层（如拖拽手柄） */
  overlayBottomRight?: React.ReactNode;
  /** 是否禁用点击（cursor-not-allowed） */
  disabled?: boolean;
}

// ============ ThumbnailCard 组件 ============

/**
 * ThumbnailCard - 壁纸缩略图卡片基础组件
 *
 * 提供统一的卡片渲染结构：
 * - aspect-video 缩略图区域（LazyImage + fallback）
 * - 底部文件名 + 类型图标
 * - 右上角类型角标（video/gif）
 * - 可插入的叠加层插槽（左上、右下）
 */
const ThumbnailCard: React.FC<ThumbnailCardProps> = ({
  wallpaper,
  onClick,
  onContextMenu,
  className,
  style,
  overlayTopLeft,
  overlayBottomRight,
  disabled = false,
}) => {
  const { t } = useTranslation();
  const TypeIcon = wallpaper.type === "video" ? Film : Image;

  return (
    <div
      className={cn(
        "group relative overflow-hidden rounded-md border border-border/40 bg-card transition-all duration-200",
        "fluent-shadow-sm hover:fluent-shadow hover:border-border/60 hover:-translate-y-0.5",
        disabled ? "cursor-not-allowed opacity-60" : "cursor-pointer",
        className,
      )}
      style={style}
      onClick={disabled ? undefined : onClick}
      onContextMenu={disabled ? undefined : onContextMenu}
    >
      {/* 左上角叠加层插槽 */}
      {overlayTopLeft}

      {/* 右下角叠加层插槽 */}
      {overlayBottomRight}

      {/* 缩略图 */}
      <div className="aspect-video">
        {wallpaper.thumb_path ? (
          <LazyImage
            src={convertFileSrc(wallpaper.thumb_path)}
            alt={wallpaper.name}
            fallback={<TypeIcon className="size-8 text-foreground/50/40" />}
          />
        ) : (
          <div className="flex size-full items-center justify-center bg-foreground/4">
            <TypeIcon className="size-8 text-foreground/50/40" />
          </div>
        )}
      </div>

      {/* 文件信息 */}
      <div className="flex items-center gap-1.5 px-2 py-1.5">
        <TypeIcon className="size-3.5 shrink-0 text-foreground/40" />
        <span className="truncate text-xs text-foreground/70">{wallpaper.name}</span>
      </div>

      {/* 类型角标 */}
      {(wallpaper.type === "video" || wallpaper.type === "gif") && (
        <div className="absolute right-1.5 top-1.5 rounded-sm bg-black/50 px-1.5 py-0.5 text-[10px] font-medium text-white/90 backdrop-blur-sm">
          {t(`preview.${wallpaper.type}`)}
        </div>
      )}
    </div>
  );
};

export default memo(ThumbnailCard);