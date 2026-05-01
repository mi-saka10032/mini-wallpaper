import { useCallback, useMemo, useState } from "react";
import { Check, Plus, RotateCcw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Slider } from "@/components/ui/slider";
import { useAccentColor, ACCENT_PRESETS } from "@/hooks/useAccentColor";

/**
 * 获取预设色块的预览颜色（用于展示）
 */
function getPresetPreviewColor(hue: number, chroma: number, isDefault: boolean): string {
  if (isDefault) {
    return "oklch(0.5 0 0)"; // 灰色
  }
  return `oklch(0.6 ${chroma} ${hue})`;
}

const AccentColorPicker: React.FC = () => {
  const { t } = useTranslation();
  const { accentValue, setAccentColor, setCustomColor, currentConfig } = useAccentColor();

  const [customOpen, setCustomOpen] = useState(false);
  const [customHue, setCustomHue] = useState(currentConfig.hue || 250);
  const [customChroma, setCustomChroma] = useState(
    currentConfig.chroma > 0 ? currentConfig.chroma : 0.15,
  );

  // 判断当前选中的是哪个预设
  const activePresetId = useMemo(() => {
    if (accentValue.startsWith("custom:")) return null;
    return accentValue || "default";
  }, [accentValue]);

  const handlePresetClick = useCallback(
    (presetId: string) => {
      setAccentColor(presetId);
    },
    [setAccentColor],
  );

  const handleCustomConfirm = useCallback(() => {
    setCustomColor(customHue, customChroma);
    setCustomOpen(false);
  }, [customHue, customChroma, setCustomColor]);

  const handleReset = useCallback(() => {
    setAccentColor("default");
    setCustomOpen(false);
  }, [setAccentColor]);

  // 自定义颜色预览
  const customPreviewColor = useMemo(
    () => `oklch(0.6 ${customChroma} ${customHue})`,
    [customHue, customChroma],
  );

  // 当前是否为自定义颜色
  const isCustomActive = accentValue.startsWith("custom:");

  return (
    <div className="space-y-3">
      <Label className="text-sm font-medium">{t("settings.accentColor")}</Label>

      <div className="flex flex-wrap items-center gap-2">
        {/* 预设色块 */}
        {ACCENT_PRESETS.map((preset) => {
          const isActive = activePresetId === preset.id;
          const previewColor = getPresetPreviewColor(
            preset.hue,
            preset.chroma,
            preset.id === "default",
          );

          return (
            <button
              key={preset.id}
              type="button"
              onClick={() => handlePresetClick(preset.id)}
              className={cn(
                "group relative flex size-7 items-center justify-center rounded-full transition-all duration-200",
                "ring-offset-background hover:scale-110",
                isActive && "ring-2 ring-ring ring-offset-2",
              )}
              style={{ backgroundColor: previewColor }}
              title={t(preset.label)}
            >
              {isActive && (
                <Check
                  className="size-3.5 drop-shadow-sm"
                  style={{
                    color: preset.id === "default" ? "white" : "white",
                    filter: "drop-shadow(0 1px 1px rgba(0,0,0,0.3))",
                  }}
                />
              )}
            </button>
          );
        })}

        {/* 自定义颜色按钮 */}
        <Popover open={customOpen} onOpenChange={setCustomOpen}>
          <PopoverTrigger asChild>
            <button
              type="button"
              className={cn(
                "relative flex size-7 items-center justify-center rounded-full transition-all duration-200",
                "border-2 border-dashed border-foreground/25 hover:border-foreground/50 hover:scale-110",
                "ring-offset-background",
                isCustomActive && "ring-2 ring-ring ring-offset-2 border-solid",
              )}
              style={
                isCustomActive
                  ? { backgroundColor: `oklch(0.6 ${currentConfig.chroma} ${currentConfig.hue})`, borderColor: "transparent" }
                  : undefined
              }
              title={t("accentColor.custom")}
            >
              {isCustomActive ? (
                <Check
                  className="size-3.5"
                  style={{ color: "white", filter: "drop-shadow(0 1px 1px rgba(0,0,0,0.3))" }}
                />
              ) : (
                <Plus className="size-3.5 text-foreground/50" />
              )}
            </button>
          </PopoverTrigger>

          <PopoverContent className="w-64" align="start" sideOffset={8}>
            <div className="space-y-4">
              <div className="flex items-center justify-between">
                <span className="text-sm font-medium">{t("accentColor.customTitle")}</span>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={handleReset}
                  className="h-7 gap-1 px-2 text-xs text-foreground/50"
                >
                  <RotateCcw className="size-3" />
                  {t("accentColor.reset")}
                </Button>
              </div>

              {/* 颜色预览 */}
              <div className="flex items-center gap-3">
                <div
                  className="size-10 rounded-lg shadow-inner ring-1 ring-black/10"
                  style={{ backgroundColor: customPreviewColor }}
                />
                <div className="flex-1 space-y-0.5">
                  <p className="text-xs text-foreground/50">
                    {t("accentColor.hue")}: {Math.round(customHue)}°
                  </p>
                  <p className="text-xs text-foreground/50">
                    {t("accentColor.saturation")}: {Math.round(customChroma * 100)}%
                  </p>
                </div>
              </div>

              {/* 色相滑块 */}
              <div className="space-y-2">
                <label className="text-xs text-foreground/50">
                  {t("accentColor.hue")}
                </label>
                <div className="relative">
                  <div
                    className="absolute inset-0 rounded-full"
                    style={{
                      background:
                        "linear-gradient(to right, oklch(0.6 0.15 0), oklch(0.6 0.15 60), oklch(0.6 0.15 120), oklch(0.6 0.15 180), oklch(0.6 0.15 240), oklch(0.6 0.15 300), oklch(0.6 0.15 360))",
                      height: "8px",
                      top: "50%",
                      transform: "translateY(-50%)",
                    }}
                  />
                  <Slider
                    value={[customHue]}
                    onValueChange={(v) => setCustomHue(v[0])}
                    min={0}
                    max={360}
                    step={1}
                    className="relative"
                  />
                </div>
              </div>

              {/* 饱和度滑块 */}
              <div className="space-y-2">
                <label className="text-xs text-foreground/50">
                  {t("accentColor.saturation")}
                </label>
                <div className="relative">
                  <div
                    className="absolute inset-0 rounded-full"
                    style={{
                      background: `linear-gradient(to right, oklch(0.6 0 ${customHue}), oklch(0.6 0.2 ${customHue}))`,
                      height: "8px",
                      top: "50%",
                      transform: "translateY(-50%)",
                    }}
                  />
                  <Slider
                    value={[customChroma * 100]}
                    onValueChange={(v) => setCustomChroma(v[0] / 100)}
                    min={5}
                    max={25}
                    step={1}
                    className="relative"
                  />
                </div>
              </div>

              {/* 确认按钮 */}
              <Button
                size="sm"
                className="w-full"
                onClick={handleCustomConfirm}
              >
                {t("accentColor.apply")}
              </Button>
            </div>
          </PopoverContent>
        </Popover>
      </div>

      {/* 当前选中提示 */}
      <p className="text-xs text-foreground/50">
        {isCustomActive
          ? t("accentColor.customActive")
          : t(ACCENT_PRESETS.find((p) => p.id === (activePresetId || "default"))?.label || "accentColor.default")}
      </p>
    </div>
  );
};

export default AccentColorPicker;
