import { memo, useCallback, useMemo, type FC } from "react";
import { useTranslation } from "react-i18next";
import { AlertCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { SETTING_KEYS } from "@/stores/settingStore";
import { DEFAULT_SHORTCUTS } from "@/constants/shortcuts";
import { useShortcutRecorder } from "@/hooks/useShortcutRecorder";

interface ShortcutsSectionProps {
  shortcutNext: string;
  shortcutPrev: string;
  shortcutTogglePause: string;
  shortcutOpenMain: string;
  shortcutToggleFavorite: string;
  updateSetting: (key: string, value: string) => void;
}

/** 快捷键配置项定义 */
interface ShortcutItem {
  settingKey: string;
  labelKey: string;
  defaultValue: string;
  currentValue: string;
}

/** 快捷键设置区块 - Win11 Fluent 风格，支持冲突检测 */
const ShortcutsSection: FC<ShortcutsSectionProps> = memo(({
  shortcutNext,
  shortcutPrev,
  shortcutTogglePause,
  shortcutOpenMain,
  shortcutToggleFavorite,
  updateSetting,
}) => {
  const { t } = useTranslation();
  const {
    recordingAction,
    pendingShortcut,
    conflictKey,
    recorderRef,
    handleRecordKeyDown,
    handleRecordKeyUp,
    startRecording,
    resetShortcut,
    cancelRecording,
    formatShortcut,
  } = useShortcutRecorder(updateSetting);

  /** 所有快捷键配置项 */
  const shortcutItems: ShortcutItem[] = useMemo(() => [
    {
      settingKey: SETTING_KEYS.SHORTCUT_NEXT,
      labelKey: "settings.shortcutNext",
      defaultValue: DEFAULT_SHORTCUTS.nextWallpaper,
      currentValue: shortcutNext,
    },
    {
      settingKey: SETTING_KEYS.SHORTCUT_PREV,
      labelKey: "settings.shortcutPrev",
      defaultValue: DEFAULT_SHORTCUTS.prevWallpaper,
      currentValue: shortcutPrev,
    },
    {
      settingKey: SETTING_KEYS.SHORTCUT_TOGGLE_PAUSE,
      labelKey: "settings.shortcutTogglePause",
      defaultValue: DEFAULT_SHORTCUTS.togglePause,
      currentValue: shortcutTogglePause,
    },
    {
      settingKey: SETTING_KEYS.SHORTCUT_OPEN_MAIN,
      labelKey: "settings.shortcutOpenMain",
      defaultValue: DEFAULT_SHORTCUTS.openMain,
      currentValue: shortcutOpenMain,
    },
    {
      settingKey: SETTING_KEYS.SHORTCUT_TOGGLE_FAVORITE,
      labelKey: "settings.shortcutToggleFavorite",
      defaultValue: DEFAULT_SHORTCUTS.toggleFavorite,
      currentValue: shortcutToggleFavorite,
    },
  ], [shortcutNext, shortcutPrev, shortcutTogglePause, shortcutOpenMain, shortcutToggleFavorite]);

  /** 获取所有当前快捷键值（用于冲突检测） */
  const allShortcutValues = useMemo(() => {
    const map: Record<string, string> = {};
    for (const item of shortcutItems) {
      map[item.settingKey] = item.currentValue;
    }
    return map;
  }, [shortcutItems]);

  /** 带冲突检测的开始录制 */
  const handleStartRecording = useCallback((settingKey: string) => {
    startRecording(settingKey, allShortcutValues);
  }, [startRecording, allShortcutValues]);

  return (
    <section className="space-y-5">
      <div>
        <h3 className="text-[15px] font-semibold text-foreground">
          {t("settings.navShortcuts")}
        </h3>
        <p className="mt-1 text-[11px] leading-relaxed text-foreground/45">
          {t("settings.shortcutsDesc")}
        </p>
      </div>

      {/* 快捷键卡片 */}
      <div className="rounded-lg border border-border/50 bg-card">
        {shortcutItems.map((item, index) => (
          <div key={item.settingKey}>
            {index > 0 && <div className="mx-4 h-px bg-border/30" />}
            <div className="px-4 py-3.5">
              <Label className="text-[13px] font-medium">{t(item.labelKey)}</Label>
              <div className="mt-2 flex items-center gap-2">
                {recordingAction === item.settingKey ? (
                  <div className="flex flex-col gap-1 flex-1 max-w-[200px]">
                    <div
                      ref={recorderRef}
                      className={`flex h-8 items-center rounded-md border px-3 text-[13px] outline-none ${
                        conflictKey
                          ? "border-destructive/60 bg-destructive/5 text-destructive"
                          : "border-primary/60 bg-primary/5 text-primary animate-pulse"
                      }`}
                      tabIndex={0}
                      onKeyDown={handleRecordKeyDown}
                      onKeyUp={handleRecordKeyUp}
                      onBlur={cancelRecording}
                    >
                      {pendingShortcut ? formatShortcut(pendingShortcut) : t("settings.shortcutRecording")}
                    </div>
                    {conflictKey && (
                      <div className="flex items-center gap-1 text-[11px] text-destructive/80">
                        <AlertCircle className="size-3" />
                        <span>{t("settings.shortcutConflict", { action: t(shortcutItems.find(i => i.settingKey === conflictKey)?.labelKey || "") })}</span>
                      </div>
                    )}
                  </div>
                ) : (
                  <button
                    type="button"
                    onClick={() => handleStartRecording(item.settingKey)}
                    className="flex h-8 flex-1 items-center rounded-md border border-border/60 bg-background px-3 text-[13px] transition-colors hover:border-foreground/30 max-w-[200px]"
                  >
                    {formatShortcut(item.currentValue)}
                  </button>
                )}
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => resetShortcut(item.settingKey, item.defaultValue)}
                  className="h-8 text-[11px] text-foreground/45 hover:text-foreground/70"
                >
                  {t("settings.shortcutReset")}
                </Button>
              </div>
            </div>
          </div>
        ))}
      </div>
    </section>
  );
});

ShortcutsSection.displayName = "ShortcutsSection";

export default ShortcutsSection;