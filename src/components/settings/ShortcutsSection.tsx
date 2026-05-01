import React from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { SETTING_KEYS } from "@/stores/settingStore";
import { DEFAULT_SHORTCUTS } from "@/hooks/useShortcuts";
import { useShortcutRecorder } from "@/hooks/useGlobalSettings";

interface ShortcutsSectionProps {
  shortcutNext: string;
  shortcutPrev: string;
  updateSetting: (key: string, value: string) => void;
}

/** 快捷键设置区块 */
const ShortcutsSection: React.FC<ShortcutsSectionProps> = React.memo(({
  shortcutNext,
  shortcutPrev,
  updateSetting,
}) => {
  const { t } = useTranslation();
  const {
    recordingAction,
    pendingShortcut,
    recorderRef,
    handleRecordKeyDown,
    handleRecordKeyUp,
    startRecording,
    resetShortcut,
    cancelRecording,
    formatShortcut,
  } = useShortcutRecorder(updateSetting);

  return (
    <div className="space-y-6">
      <h3 className="text-base font-semibold">{t("settings.navShortcuts")}</h3>
      <p className="text-xs text-muted-foreground">
        {t("settings.shortcutsDesc")}
      </p>

      {/* 下一张壁纸 */}
      <div className="space-y-2">
        <Label className="text-sm font-medium">{t("settings.shortcutNext")}</Label>
        <div className="flex items-center gap-2">
          {recordingAction === SETTING_KEYS.SHORTCUT_NEXT ? (
            <div
              ref={recorderRef}
              className="flex h-9 flex-1 items-center rounded-md border border-primary bg-muted/50 px-3 text-sm text-primary animate-pulse max-w-xs outline-none"
              tabIndex={0}
              onKeyDown={handleRecordKeyDown}
              onKeyUp={handleRecordKeyUp}
              onBlur={cancelRecording}
            >
              {pendingShortcut ? formatShortcut(pendingShortcut) : t("settings.shortcutRecording")}
            </div>
          ) : (
            <button
              type="button"
              onClick={() => startRecording(SETTING_KEYS.SHORTCUT_NEXT)}
              className="flex h-9 flex-1 items-center rounded-md border border-border bg-background px-3 text-sm transition-colors hover:border-primary max-w-xs"
            >
              {formatShortcut(shortcutNext)}
            </button>
          )}
          <Button
            variant="ghost"
            size="sm"
            onClick={() => resetShortcut(SETTING_KEYS.SHORTCUT_NEXT, DEFAULT_SHORTCUTS.nextWallpaper)}
            className="text-xs text-muted-foreground"
          >
            {t("settings.shortcutReset")}
          </Button>
        </div>
      </div>

      <Separator />

      {/* 上一张壁纸 */}
      <div className="space-y-2">
        <Label className="text-sm font-medium">{t("settings.shortcutPrev")}</Label>
        <div className="flex items-center gap-2">
          {recordingAction === SETTING_KEYS.SHORTCUT_PREV ? (
            <div
              ref={recorderRef}
              className="flex h-9 flex-1 items-center rounded-md border border-primary bg-muted/50 px-3 text-sm text-primary animate-pulse max-w-xs outline-none"
              tabIndex={0}
              onKeyDown={handleRecordKeyDown}
              onKeyUp={handleRecordKeyUp}
              onBlur={cancelRecording}
            >
              {pendingShortcut ? formatShortcut(pendingShortcut) : t("settings.shortcutRecording")}
            </div>
          ) : (
            <button
              type="button"
              onClick={() => startRecording(SETTING_KEYS.SHORTCUT_PREV)}
              className="flex h-9 flex-1 items-center rounded-md border border-border bg-background px-3 text-sm transition-colors hover:border-primary max-w-xs"
            >
              {formatShortcut(shortcutPrev)}
            </button>
          )}
          <Button
            variant="ghost"
            size="sm"
            onClick={() => resetShortcut(SETTING_KEYS.SHORTCUT_PREV, DEFAULT_SHORTCUTS.prevWallpaper)}
            className="text-xs text-muted-foreground"
          >
            {t("settings.shortcutReset")}
          </Button>
        </div>
      </div>
    </div>
  );
});

ShortcutsSection.displayName = "ShortcutsSection";

export default ShortcutsSection;
