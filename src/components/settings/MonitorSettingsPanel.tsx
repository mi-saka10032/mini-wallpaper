import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Monitor as MonitorIcon, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useMonitorSettings } from "@/hooks/useMonitorSettings";
import { useMonitorConfig } from "@/hooks/useMonitorConfig";
import MonitorVisualizer from "./MonitorVisualizer";
import DisplayModeSection from "./DisplayModeSection";
import WallpaperSourceSection from "./WallpaperSourceSection";
import RotationSection from "./RotationSection";

const MonitorSettingsPanel: React.FC = () => {
  const { t } = useTranslation();

  const {
    loading,
    activeConfigs,
    selectedIndex,
    selectedConfig,
    selectedMonitorId,
    displayMode,
    isSyncMode,
    collections,
    wallpapers,
    intervalSliderValue,
    setSelectedIndex,
    getWallpaperThumb,
    handleDisplayModeChange,
    handleRefresh,
  } = useMonitorSettings();

  const {
    collectionWarning,
    sourceType,
    syncSourceType,
    handleSourceChange,
    handleWallpaperSelect,
    handleCollectionSelect,
    handleFitModeChange,
    handlePlayModeChange,
    handleIntervalChange,
    handleEnabledChange,
  } = useMonitorConfig({ selectedMonitorId, isSyncMode });

  // 当选中显示器或配置变化时同步 sourceType
  useEffect(() => {
    syncSourceType(!!selectedConfig?.collection_id);
  }, [selectedConfig?.collection_id, selectedMonitorId, syncSourceType]);

  return (
    <div className="flex h-full flex-col">
      {/* 标题栏 */}
      <div className="flex items-center justify-between border-b border-border/40 px-6 py-1">
        <div className="flex items-center gap-2">
          <MonitorIcon className="size-5 text-foreground/50" />
          <h2 className="text-base font-semibold">{t("monitor.title")}</h2>
        </div>
        <Button variant="ghost" size="sm" onClick={handleRefresh} className="text-foreground/60 hover:text-foreground hover:bg-primary-hover">
          <RefreshCw className="mr-1.5 size-3.5" />
          {t("monitor.refresh")}
        </Button>
      </div>

      <div className="flex-1 overflow-y-auto">
        <div className="space-y-6 px-6 py-5">
          {/* ===== 显示器可视化 ===== */}
          <MonitorVisualizer
            loading={loading}
            activeConfigs={activeConfigs}
            selectedIndex={selectedIndex}
            isSyncMode={isSyncMode}
            getWallpaperThumb={getWallpaperThumb}
            onSelectMonitor={setSelectedIndex}
          />

          {/* ===== 显示模式 ===== */}
          {selectedConfig && activeConfigs.length > 0 && (
            <>
              <Separator />
              <DisplayModeSection
                displayMode={displayMode}
                isSyncMode={isSyncMode}
                onDisplayModeChange={handleDisplayModeChange}
              />
            </>
          )}

          {selectedConfig && (
            <>
              <Separator />

              {/* ===== 壁纸来源 ===== */}
              <WallpaperSourceSection
                sourceType={sourceType}
                wallpapers={wallpapers}
                collections={collections}
                selectedWallpaperId={selectedConfig?.wallpaper_id ?? null}
                selectedCollectionId={selectedConfig?.collection_id ?? null}
                collectionWarning={collectionWarning}
                onSourceChange={handleSourceChange}
                onWallpaperSelect={handleWallpaperSelect}
                onCollectionSelect={handleCollectionSelect}
              />

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
                  <RotationSection
                    isEnabled={selectedConfig?.is_enabled ?? false}
                    playMode={selectedConfig?.play_mode ?? "sequential"}
                    intervalSliderValue={intervalSliderValue}
                    playInterval={selectedConfig?.play_interval ?? 300}
                    onEnabledChange={handleEnabledChange}
                    onPlayModeChange={handlePlayModeChange}
                    onIntervalChange={handleIntervalChange}
                  />
                </>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
};

export default MonitorSettingsPanel;
