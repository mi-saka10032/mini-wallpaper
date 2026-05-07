import { memo, useMemo, type FC } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { Monitor as MonitorIcon, MonitorSmartphone, Link } from "lucide-react";
import { cn } from "@/lib/utils";
import type { Wallpaper, MonitorConfig } from "@/api/config";

interface MonitorVisualizerProps {
  loading: boolean;
  activeConfigs: MonitorConfig[];
  selectedIndex: number;
  isSyncMode: boolean;
  getWallpaperThumb: (wallpaperId: number | null) => Wallpaper | null;
  onSelectMonitor: (index: number) => void;
}

/** 显示器可视化布局组件 */
const MonitorVisualizer: FC<MonitorVisualizerProps> = memo(({
  loading,
  activeConfigs,
  selectedIndex,
  isSyncMode,
  getWallpaperThumb,
  onSelectMonitor,
}) => {
  const { t } = useTranslation();

  const monitorLayout = useMemo(() => {
    if (activeConfigs.length === 0) return { items: [], containerWidth: 0, containerHeight: 0 };

    const maxW = 500;
    const gap = 16;
    const perW = Math.min(200, (maxW - gap * (activeConfigs.length - 1)) / activeConfigs.length);
    const perH = perW * 0.625; // 16:10 ratio

    const items = activeConfigs.map((c, i) => ({
      index: i,
      monitorId: c.monitor_id,
      x: i * (perW + gap),
      y: 0,
      width: perW,
      height: perH,
    }));

    return {
      items,
      containerWidth: activeConfigs.length * perW + (activeConfigs.length - 1) * gap,
      containerHeight: perH,
    };
  }, [activeConfigs]);

  if (loading && activeConfigs.length === 0) {
    return (
      <div className="flex items-center justify-center py-12 text-foreground/50">
        <MonitorSmartphone className="mr-2 size-5" />
        {t("monitor.detecting")}
      </div>
    );
  }

  if (activeConfigs.length === 0) {
    return (
      <div className="flex items-center justify-center py-12 text-foreground/50">
        <MonitorSmartphone className="mr-2 size-5" />
        {t("monitor.noMonitor")}
      </div>
    );
  }

  const selectedConfig = activeConfigs[selectedIndex] ?? null;

  return (
    <div className="flex flex-col items-center gap-4">
      {/* 显示器图形 */}
      <div
        className="relative"
        style={{
          width: monitorLayout.containerWidth + 40,
          height: monitorLayout.containerHeight + 60,
        }}
      >
        {monitorLayout.items.map((item) => {
          const config = activeConfigs[item.index];
          const wp = getWallpaperThumb(config?.wallpaper_id ?? null);
          const isSelected = item.index === selectedIndex;
          const isDisabled = isSyncMode && !isSelected;

          return (
            <button
              key={item.monitorId}
              type="button"
              disabled={isDisabled}
              onClick={() => !isDisabled && onSelectMonitor(item.index)}
              className={cn(
                "absolute flex flex-col items-center transition-all duration-200",
                isDisabled && "cursor-not-allowed opacity-50",
              )}
              style={{
                left: item.x + 20,
                top: item.y + 10,
                width: item.width,
              }}
            >
              {/* 显示器屏幕 */}
              <div
                className={cn(
                  "relative overflow-hidden rounded-lg border-2 bg-foreground/4 transition-colors",
                  isSelected
                    ? "border-primary shadow-lg shadow-primary/20"
                    : isDisabled
                      ? "border-border/50"
                      : "border-border hover:border-foreground/40",
                )}
                style={{
                  width: item.width,
                  height: item.height,
                }}
              >
                {/* 壁纸预览（跟随适配模式） */}
                {wp?.thumb_path ? (
                  <img
                    src={convertFileSrc(wp.thumb_path)}
                    alt=""
                    className={cn("size-full", {
                      "object-cover": config?.fit_mode === "cover",
                      "object-contain": config?.fit_mode === "contain",
                      "object-fill": config?.fit_mode === "fill",
                      "object-none": config?.fit_mode === "center",
                    })}
                  />
                ) : (
                  <div className="flex size-full items-center justify-center">
                    <MonitorIcon className="size-8 text-foreground/50/30" />
                  </div>
                )}

                {/* 序号 */}
                <div
                  className={cn(
                    "absolute left-1.5 top-1.5 flex size-5 items-center justify-center rounded-full text-[10px] font-bold",
                    isSelected
                      ? "bg-primary text-primary-foreground"
                      : "bg-background/80 text-foreground/50",
                  )}
                >
                  {item.index + 1}
                </div>

                {/* 同步模式下非选中显示器显示已同步徽章 */}
                {isSyncMode && !isSelected && (
                  <div className="absolute bottom-1 right-1 flex items-center gap-0.5 rounded-full bg-primary/80 px-1.5 py-0.5 text-[8px] font-medium text-primary-foreground">
                    <Link className="size-2.5" />
                    {t("monitor.displaySyncedBadge")}
                  </div>
                )}
              </div>

              {/* 底座 */}
              <div
                className={cn(
                  "mt-0.5 h-1.5 rounded-b-sm",
                  isSelected ? "bg-primary/60" : "bg-border",
                )}
                style={{ width: Math.max(item.width * 0.3, 20) }}
              />
              <div
                className={cn(
                  "h-1 rounded-b-sm",
                  isSelected ? "bg-primary/40" : "bg-border/60",
                )}
                style={{ width: Math.max(item.width * 0.15, 12) }}
              />

              {/* 名称 */}
              <span
                className={cn(
                  "mt-1 max-w-full truncate text-[10px]",
                  isSelected ? "font-medium text-foreground" : "text-foreground/50",
                )}
              >
                {item.monitorId}
              </span>
            </button>
          );
        })}
      </div>

      {/* 选中显示器信息 */}
      {selectedConfig && (
        <div className="text-center text-xs text-foreground/50">
          {selectedConfig.monitor_id}
          {selectedConfig.is_enabled && selectedConfig.collection_id
            ? ` · ${t("monitor.rotating")}`
            : ""}
        </div>
      )}
    </div>
  );
});

MonitorVisualizer.displayName = "MonitorVisualizer";

export default MonitorVisualizer;
