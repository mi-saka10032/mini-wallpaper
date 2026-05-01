import React from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { SETTING_KEYS } from "@/stores/settingStore";
import { DEFAULT_SHORTCUTS } from "@/hooks/useShortcuts";
import { useShortcutRecorder } from "@/hooks/useShortcutRecorder";

interface ShortcutsSectionProps {
  shortcutNext: string;
  shortcutPrev: string;
  updateSetting: (key: string, value: string) => void;
}

/** 快捷键设置区块 - Win11 Fluent 风格 */
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
        {/* 下一张壁纸 */}
        <div className="px-4 py-3.5">
          <Label className="text-[13px] font-medium">{t("settings.shortcutNext")}</Label>
          <div className="mt-2 flex items-center gap-2">
            {recordingAction === SETTING_KEYS.SHORTCUT_NEXT ? (
              <div
                ref={recorderRef}
                className="flex h-8 flex-1 items-center rounded-md border border-primary/60 bg-primary/5 px-3 text-[13px] text-primary animate-pulse max-w-[200px] outline-none"
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
                className="flex h-8 flex-1 items-center rounded-md border border-border/60 bg-background px-3 text-[13px] transition-colors hover:border-foreground/30 max-w-[200px]"
              >
                {formatShortcut(shortcutNext)}
              </button>
            )}
            <Button
              variant="ghost"
              size="sm"
              onClick={() => resetShortcut(SETTING_KEYS.SHORTCUT_NEXT, DEFAULT_SHORTCUTS.nextWallpaper)}
              className="h-8 text-[11px] text-foreground/45 hover:text-foreground/70"
            >
              {t("settings.shortcutReset")}
            </Button>
          </div>
        </div>

        <div className="mx-4 h-px bg-border/30" />

        {/* 上一张壁纸 */}
        <div className="px-4 py-3.5">
          <Label className="text-[13px] font-medium">{t("settings.shortcutPrev")}</Label>
          <div className="mt-2 flex items-center gap-2">
            {recordingAction === SETTING_KEYS.SHORTCUT_PREV ? (
              <div
                ref={recorderRef}
                className="flex h-8 flex-1 items-center rounded-md border border-primary/60 bg-primary/5 px-3 text-[13px] text-primary animate-pulse max-w-[200px] outline-none"
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
                className="flex h-8 flex-1 items-center rounded-md border border-border/60 bg-background px-3 text-[13px] transition-colors hover:border-foreground/30 max-w-[200px]"
              >
                {formatShortcut(shortcutPrev)}
              </button>
            )}
            <Button
              variant="ghost"
              size="sm"
              onClick={() => resetShortcut(SETTING_KEYS.SHORTCUT_PREV, DEFAULT_SHORTCUTS.prevWallpaper)}
              className="h-8 text-[11px] text-foreground/45 hover:text-foreground/70"
            >
              {t("settings.shortcutReset")}
            </Button>
          </div>
        </div>
      </div>
    </section>
  );
});

ShortcutsSection.displayName = "ShortcutsSection";

export default ShortcutsSection;
