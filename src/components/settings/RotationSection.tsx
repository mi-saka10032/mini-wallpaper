import React from "react";
import { useTranslation } from "react-i18next";
import { ArrowRight, Shuffle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import { formatInterval, INTERVAL_PRESETS } from "@/hooks/useMonitorSettings";

interface RotationSectionProps {
  isEnabled: boolean;
  playMode: string;
  intervalSliderValue: number;
  playInterval: number;
  onEnabledChange: (enabled: boolean) => void;
  onPlayModeChange: (mode: string) => void;
  onIntervalChange: (value: number[]) => void;
}

/** 轮播设置区块 */
const RotationSection: React.FC<RotationSectionProps> = React.memo(({
  isEnabled,
  playMode,
  intervalSliderValue,
  playInterval,
  onEnabledChange,
  onPlayModeChange,
  onIntervalChange,
}) => {
  const { t } = useTranslation();

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <Label className="text-sm font-medium">{t("monitor.rotationSettings")}</Label>
        <div className="flex items-center gap-2">
          <span className="text-xs text-muted-foreground">
            {isEnabled ? t("monitor.enabled") : t("monitor.paused")}
          </span>
          <Switch
            checked={isEnabled}
            onCheckedChange={onEnabledChange}
          />
        </div>
      </div>

      {/* 播放模式 */}
      <div className="space-y-2">
        <Label className="text-xs text-muted-foreground">{t("monitor.playMode")}</Label>
        <div className="flex gap-2">
          <Button
            variant={(playMode ?? "sequential") === "sequential" ? "default" : "outline"}
            size="sm"
            onClick={() => onPlayModeChange("sequential")}
          >
            <ArrowRight className="mr-1.5 size-3.5" />
            {t("monitor.sequential")}
          </Button>
          <Button
            variant={playMode === "random" ? "default" : "outline"}
            size="sm"
            onClick={() => onPlayModeChange("random")}
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
            {formatInterval(playInterval, t)}
          </span>
        </div>
        <Slider
          value={[intervalSliderValue]}
          onValueChange={onIntervalChange}
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
  );
});

RotationSection.displayName = "RotationSection";

export default RotationSection;
