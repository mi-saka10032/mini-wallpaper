import { useCallback, useEffect, useMemo, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import {
  Monitor as MonitorIcon,
  MonitorSmartphone,
  Shuffle,
  ArrowRight,
  Image,
  FolderOpen,
  RefreshCw,
  AlertTriangle,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";
import { useMonitorConfigStore } from "@/stores/monitorConfigStore";
import { useCollectionStore } from "@/stores/collectionStore";
import { useWallpaperStore } from "@/stores/wallpaperStore";
import { getWallpapers as getCollectionWallpapers } from "@/api/collection";
import type { Wallpaper } from "@/api/config";

/** 格式化间隔显示 */
function formatInterval(seconds: number, t: (key: string, opts?: Record<string, unknown>) => string): string {
  if (seconds < 60) return t("time.seconds", { count: seconds });
  if (seconds < 3600) return t("time.minutes", { count: Math.round(seconds / 60) });
  return t("time.hours", { count: Math.round(seconds / 3600) });
}

/** 间隔预设值（秒） */
const INTERVAL_PRESETS = [10, 30, 60, 300, 600, 1800, 3600, 7200];

const MonitorSettingsPanel: React.FC = () => {
  const { t } = useTranslation();

  // Store
  const configs = useMonitorConfigStore((s) => s.configs);
  const init = useMonitorConfigStore((s) => s.init);
  const upsert = useMonitorConfigStore((s) => s.upsert);
  const loading = useMonitorConfigStore((s) => s.loading);
  const collections = useCollectionStore((s) => s.collections);
  const fetchCollections = useCollectionStore((s) => s.fetchCollections);
  const wallpapers = useWallpaperStore((s) => s.wallpapers);

  // 只使用 active 的 config 作为显示器列表
  const activeConfigs = useMemo(
    () => configs.filter((c) => c.active),
    [configs],
  );

  // 选中的 monitor index
  const [selectedIndex, setSelectedIndex] = useState<number>(0);

  useEffect(() => {
    init();
    fetchCollections();
  }, [init, fetchCollections]);

  // 选中范围保护
  useEffect(() => {
    if (activeConfigs.length > 0 && selectedIndex >= activeConfigs.length) {
      setSelectedIndex(0);
    }
  }, [activeConfigs.length, selectedIndex]);

  const selectedConfig = activeConfigs[selectedIndex] ?? null;
  const selectedMonitorId = selectedConfig?.monitor_id ?? null;

  // 获取壁纸缩略图
  const getWallpaperThumb = useCallback(
    (wallpaperId: number | null): Wallpaper | null => {
      if (!wallpaperId) return null;
      return wallpapers.find((w) => w.id === wallpaperId) ?? null;
    },
    [wallpapers],
  );

  // ===== 显示器可视化布局 =====
  // 由于我们不再持有 TauriMonitor 的 position/size，
  // 简化为等分横排布局
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

  // ===== 配置更新 =====
  const [collectionWarning, setCollectionWarning] = useState<string | null>(null);
  const [sourceType, setSourceType] = useState<"wallpaper" | "collection">("wallpaper");

  // 当选中显示器或配置变化时同步 sourceType
  useEffect(() => {
    if (selectedConfig?.collection_id) {
      setSourceType("collection");
    } else {
      setSourceType("wallpaper");
    }
  }, [selectedConfig?.collection_id, selectedMonitorId]);

  const handleSourceChange = useCallback(
    async (source: "wallpaper" | "collection") => {
      if (!selectedMonitorId) return;
      setCollectionWarning(null);
      setSourceType(source);

      if (source === "wallpaper" && selectedConfig?.collection_id) {
        await upsert({ monitorId: selectedMonitorId, clearCollection: true });
      }
    },
    [upsert, selectedMonitorId, selectedConfig?.collection_id],
  );

  const handleWallpaperSelect = useCallback(
    async (wallpaperId: number) => {
      if (!selectedMonitorId) return;
      await upsert({ monitorId: selectedMonitorId, wallpaperId });
    },
    [upsert, selectedMonitorId],
  );

  const handleCollectionSelect = useCallback(
    async (collectionIdStr: string) => {
      if (!selectedMonitorId) return;
      const collectionId = Number(collectionIdStr);
      setCollectionWarning(null);

      try {
        const wallpapersInCollection = await getCollectionWallpapers(collectionId);

        if (wallpapersInCollection.length === 0) {
          setCollectionWarning(t("monitor.collectionEmptyWarn"));
          return;
        }

        const firstWallpaperId = wallpapersInCollection[0].id;
        await upsert({
          monitorId: selectedMonitorId,
          collectionId,
          wallpaperId: firstWallpaperId,
        });
      } catch (e) {
        console.error("[handleCollectionSelect]", e);
        setCollectionWarning(t("monitor.collectionQueryFail"));
      }
    },
    [upsert, selectedMonitorId],
  );

  const handleFitModeChange = useCallback(
    async (fitMode: string) => {
      if (!selectedMonitorId) return;
      await upsert({ monitorId: selectedMonitorId, fitMode });
    },
    [upsert, selectedMonitorId],
  );

  const handlePlayModeChange = useCallback(
    async (playMode: string) => {
      if (!selectedMonitorId) return;
      await upsert({ monitorId: selectedMonitorId, playMode });
    },
    [upsert, selectedMonitorId],
  );

  const handleIntervalChange = useCallback(
    async (value: number[]) => {
      if (!selectedMonitorId) return;
      const seconds = INTERVAL_PRESETS[value[0]] ?? 300;
      await upsert({ monitorId: selectedMonitorId, playInterval: seconds });
    },
    [upsert, selectedMonitorId],
  );

  const handleEnabledChange = useCallback(
    async (enabled: boolean) => {
      if (!selectedMonitorId) return;
      await upsert({ monitorId: selectedMonitorId, isEnabled: enabled });
    },
    [upsert, selectedMonitorId],
  );

  // 间隔滑块值
  const intervalSliderValue = useMemo(() => {
    const interval = selectedConfig?.play_interval ?? 300;
    const idx = INTERVAL_PRESETS.findIndex((v) => v >= interval);
    return idx >= 0 ? idx : 4;
  }, [selectedConfig]);

  const handleRefresh = useCallback(() => {
    init();
  }, [init]);

  return (
    <div className="flex h-full flex-col">
      {/* 标题栏 */}
      <div className="flex items-center justify-between border-b border-border px-6 py-4">
        <div className="flex items-center gap-2">
          <MonitorIcon className="size-5 text-muted-foreground" />
          <h2 className="text-lg font-semibold">{t("monitor.title")}</h2>
        </div>
        <Button variant="ghost" size="sm" onClick={handleRefresh}>
          <RefreshCw className="mr-1.5 size-3.5" />
          {t("monitor.refresh")}
        </Button>
      </div>

      <ScrollArea className="flex-1 overflow-y-auto">
        <div className="space-y-6 px-6 py-5">
          {/* ===== 显示器可视化 ===== */}
          {loading && activeConfigs.length === 0 ? (
            <div className="flex items-center justify-center py-12 text-muted-foreground">
              <MonitorSmartphone className="mr-2 size-5" />
              {t("monitor.detecting")}
            </div>
          ) : activeConfigs.length === 0 ? (
            <div className="flex items-center justify-center py-12 text-muted-foreground">
              <MonitorSmartphone className="mr-2 size-5" />
              {t("monitor.noMonitor")}
            </div>
          ) : (
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

                  return (
                    <button
                      key={item.monitorId}
                      type="button"
                      onClick={() => setSelectedIndex(item.index)}
                      className={cn(
                        "absolute flex flex-col items-center transition-all duration-200",
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
                          "relative overflow-hidden rounded-lg border-2 bg-muted/80 transition-colors",
                          isSelected
                            ? "border-primary shadow-lg shadow-primary/20"
                            : "border-border hover:border-muted-foreground/50",
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
                            <MonitorIcon className="size-8 text-muted-foreground/30" />
                          </div>
                        )}

                        {/* 序号 */}
                        <div
                          className={cn(
                            "absolute left-1.5 top-1.5 flex size-5 items-center justify-center rounded-full text-[10px] font-bold",
                            isSelected
                              ? "bg-primary text-primary-foreground"
                              : "bg-background/80 text-muted-foreground",
                          )}
                        >
                          {item.index + 1}
                        </div>
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
                          isSelected ? "font-medium text-foreground" : "text-muted-foreground",
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
                <div className="text-center text-xs text-muted-foreground">
                  {selectedConfig.monitor_id}
                  {selectedConfig.is_enabled && selectedConfig.collection_id
                    ? ` · ${t("monitor.rotating")}`
                    : ""}
                </div>
              )}
            </div>
          )}

          {selectedConfig && (
            <>
              <Separator />

              {/* ===== 壁纸来源 ===== */}
              <div className="space-y-3">
                <Label className="text-sm font-medium">{t("monitor.wallpaperSource")}</Label>
                <div className="flex gap-2">
                  <Button
                    variant={sourceType === "wallpaper" ? "default" : "outline"}
                    size="sm"
                    onClick={() => handleSourceChange("wallpaper")}
                  >
                    <Image className="mr-1.5 size-3.5" />
                    {t("monitor.singleWallpaper")}
                  </Button>
                  <Button
                    variant={sourceType === "collection" ? "default" : "outline"}
                    size="sm"
                    onClick={() => handleSourceChange("collection")}
                  >
                    <FolderOpen className="mr-1.5 size-3.5" />
                    {t("monitor.collectionRotation")}
                  </Button>
                </div>

                {/* 单张壁纸选择 */}
                {sourceType === "wallpaper" && (
                  <div className="space-y-2">
                    <Select
                      value={selectedConfig?.wallpaper_id?.toString() ?? ""}
                      onValueChange={(v) => handleWallpaperSelect(Number(v))}
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
                          value={selectedConfig?.collection_id?.toString() ?? ""}
                          onValueChange={handleCollectionSelect}
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

              <Separator />

              {/* ===== 适配模式 ===== */}
              <div className="space-y-3">
                <Label className="text-sm font-medium">{t("monitor.fitMode")}</Label>
                <Select
                  value={selectedConfig?.fit_mode ?? "cover"}
                  onValueChange={handleFitModeChange}
                >
                  <SelectTrigger className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="cover">{t("monitor.fitCover")}</SelectItem>
                    <SelectItem value="contain">{t("monitor.fitContain")}</SelectItem>
                    <SelectItem value="fill">{t("monitor.fitFill")}</SelectItem>
                    <SelectItem value="center">{t("monitor.fitCenter")}</SelectItem>
                  </SelectContent>
                </Select>
              </div>

              {/* ===== 轮播设置（仅收藏夹模式） ===== */}
              {sourceType === "collection" && selectedConfig?.collection_id && (
                <>
                  <Separator />

                  <div className="space-y-4">
                    <div className="flex items-center justify-between">
                      <Label className="text-sm font-medium">{t("monitor.rotationSettings")}</Label>
                      <div className="flex items-center gap-2">
                        <span className="text-xs text-muted-foreground">
                          {selectedConfig?.is_enabled ? t("monitor.enabled") : t("monitor.paused")}
                        </span>
                        <Switch
                          checked={selectedConfig?.is_enabled ?? false}
                          onCheckedChange={handleEnabledChange}
                        />
                      </div>
                    </div>

                    {/* 播放模式 */}
                    <div className="space-y-2">
                      <Label className="text-xs text-muted-foreground">{t("monitor.playMode")}</Label>
                      <div className="flex gap-2">
                        <Button
                          variant={
                            (selectedConfig?.play_mode ?? "sequential") === "sequential"
                              ? "default"
                              : "outline"
                          }
                          size="sm"
                          onClick={() => handlePlayModeChange("sequential")}
                        >
                          <ArrowRight className="mr-1.5 size-3.5" />
                          {t("monitor.sequential")}
                        </Button>
                        <Button
                          variant={selectedConfig?.play_mode === "random" ? "default" : "outline"}
                          size="sm"
                          onClick={() => handlePlayModeChange("random")}
                        >
                          <Shuffle className="mr-1.5 size-3.5" />
                          {t("monitor.random")}
                        </Button>
                      </div>
                    </div>

                    {/* 轮播间隔 */}
                    <div className="space-y-2">
                      <div className="flex items-center justify-between">
                        <Label className="text-xs text-muted-foreground">{t("monitor.interval")}</Label>
                        <span className="text-xs font-medium text-foreground">
                          {formatInterval(selectedConfig?.play_interval ?? 300, t)}
                        </span>
                      </div>
                      <Slider
                        value={[intervalSliderValue]}
                        onValueChange={handleIntervalChange}
                        min={0}
                        max={INTERVAL_PRESETS.length - 1}
                        step={1}
                      />
                      <div className="flex justify-between text-[10px] text-muted-foreground">
                        <span>{t("time.seconds", { count: 10 })}</span>
                        <span>{t("time.hours", { count: 2 })}</span>
                      </div>
                    </div>
                  </div>
                </>
              )}
            </>
          )}
        </div>
      </ScrollArea>
    </div>
  );
};

export default MonitorSettingsPanel;
