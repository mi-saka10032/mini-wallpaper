import { useCallback, useMemo, useState } from "react";
import { Check, Palette, Plus, RotateCcw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { HslColorPicker } from "react-colorful";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { useAccentColor, ACCENT_PRESETS } from "@/hooks/useAccentColor";

/**
 * 获取预设色块的预览颜色（用于展示）
 */
function getPresetPreviewColor(hue: number, chroma: number, isDefault: boolean): string {
  if (isDefault) {
    return "oklch(0.5 0 0)";
  }
  return `oklch(0.6 ${chroma} ${hue})`;
}

/**
 * 将 oklch chroma (0~0.25) 映射到 HSL saturation (0~100)
 */
function chromaToSaturation(chroma: number): number {
  return Math.round((chroma / 0.25) * 100);
}

/**
 * 将 HSL saturation (0~100) 映射到 oklch chroma (0~0.25)
 */
function saturationToChroma(saturation: number): number {
  return Number(((saturation / 100) * 0.25).toFixed(3));
}

/**
 * Toolbar 用的换肤按钮，点击弹出 Popover 展示色块选择 + ColorPicker
 */
const AccentColorToggle: React.FC = () => {
  const { t } = useTranslation();
  const { accentValue, setAccentColor, setCustomColor, currentConfig } = useAccentColor();

  const [open, setOpen] = useState(false);
  const [customMode, setCustomMode] = useState(false);

  // HSL 状态用于 react-colorful
  const [hslColor, setHslColor] = useState({
    h: currentConfig.hue || 250,
    s: currentConfig.chroma > 0 ? chromaToSaturation(currentConfig.chroma) : 60,
    l: 50,
  });

  // 判断当前选中的是哪个预设
  const activePresetId = useMemo(() => {
    if (accentValue.startsWith("custom:")) return null;
    return accentValue || "default";
  }, [accentValue]);

  const isCustomActive = accentValue.startsWith("custom:");

  const handlePresetClick = useCallback(
    (presetId: string) => {
      setAccentColor(presetId);
      setCustomMode(false);
    },
    [setAccentColor],
  );

  const handleCustomConfirm = useCallback(() => {
    const chroma = saturationToChroma(hslColor.s);
    setCustomColor(hslColor.h, chroma);
    setCustomMode(false);
  }, [hslColor, setCustomColor]);

  const handleReset = useCallback(() => {
    setAccentColor("default");
    setCustomMode(false);
  }, [setAccentColor]);

  // 自定义颜色预览（oklch 格式）
  const customPreviewColor = useMemo(
    () => `oklch(0.6 ${saturationToChroma(hslColor.s)} ${hslColor.h})`,
    [hslColor],
  );

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="icon-sm">
          <Palette className="size-4" />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-72" align="end" sideOffset={8}>
        <div className="space-y-3">
          {/* 标题 */}
          <div className="flex items-center justify-between">
            <span className="text-sm font-medium">{t("settings.accentColor")}</span>
            <Button
              variant="ghost"
              size="sm"
              onClick={handleReset}
              className="h-6 gap-1 px-1.5 text-xs text-muted-foreground"
            >
              <RotateCcw className="size-3" />
              {t("accentColor.reset")}
            </Button>
          </div>

          {/* 预设色块网格 */}
          <div className="flex flex-wrap items-center gap-2">
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
                        color: "white",
                        filter: "drop-shadow(0 1px 1px rgba(0,0,0,0.3))",
                      }}
                    />
                  )}
                </button>
              );
            })}

            {/* 自定义颜色入口 */}
            <button
              type="button"
              onClick={() => setCustomMode(!customMode)}
              className={cn(
                "relative flex size-7 items-center justify-center rounded-full transition-all duration-200",
                "border-2 border-dashed border-muted-foreground/40 hover:border-muted-foreground hover:scale-110",
                "ring-offset-background",
                isCustomActive && "ring-2 ring-ring ring-offset-2 border-solid",
              )}
              style={
                isCustomActive
                  ? {
                      backgroundColor: `oklch(0.6 ${currentConfig.chroma} ${currentConfig.hue})`,
                      borderColor: "transparent",
                    }
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
                <Plus className="size-3.5 text-muted-foreground" />
              )}
            </button>
          </div>

          {/* 自定义颜色面板 - 使用 react-colorful */}
          {customMode && (
            <div className="space-y-3 rounded-md border border-border bg-muted/30 p-3">
              {/* ColorPicker */}
              <div className="accent-color-picker">
                <HslColorPicker color={hslColor} onChange={setHslColor} />
              </div>

              {/* 颜色预览 + 信息 */}
              <div className="flex items-center gap-3">
                <div
                  className="size-8 shrink-0 rounded-lg shadow-inner ring-1 ring-black/10"
                  style={{ backgroundColor: customPreviewColor }}
                />
                <div className="flex-1 space-y-0.5">
                  <p className="text-xs text-muted-foreground">
                    {t("accentColor.hue")}: {Math.round(hslColor.h)}°
                  </p>
                  <p className="text-xs text-muted-foreground">
                    {t("accentColor.saturation")}: {hslColor.s}%
                  </p>
                </div>
              </div>

              {/* 确认按钮 */}
              <Button size="sm" className="w-full" onClick={handleCustomConfirm}>
                {t("accentColor.apply")}
              </Button>
            </div>
          )}

          {/* 当前选中提示 */}
          <p className="text-xs text-muted-foreground">
            {isCustomActive
              ? t("accentColor.customActive")
              : t(
                  ACCENT_PRESETS.find((p) => p.id === (activePresetId || "default"))?.label ||
                    "accentColor.default",
                )}
          </p>
        </div>
      </PopoverContent>
    </Popover>
  );
};

export default AccentColorToggle;
