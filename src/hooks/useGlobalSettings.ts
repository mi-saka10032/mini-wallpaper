import { useCallback, useEffect, useState } from "react";
import { isEnabled, enable, disable } from "@tauri-apps/plugin-autostart";
import { useSettingStore, SETTING_KEYS } from "@/stores/settingStore";
import { DEFAULT_SHORTCUTS } from "@/hooks/useShortcuts";

export function useGlobalSettings() {
  const updateSetting = useSettingStore((s) => s.updateSetting);
  const volumeStr = useSettingStore((s) => s.settings[SETTING_KEYS.GLOBAL_VOLUME]);
  const closeToTray = useSettingStore((s) => s.settings[SETTING_KEYS.CLOSE_TO_TRAY]);
  const pauseOnFullscreen = useSettingStore((s) => s.settings[SETTING_KEYS.PAUSE_ON_FULLSCREEN]);
  const shortcutNext = useSettingStore((s) => s.settings[SETTING_KEYS.SHORTCUT_NEXT]) || DEFAULT_SHORTCUTS.nextWallpaper;
  const shortcutPrev = useSettingStore((s) => s.settings[SETTING_KEYS.SHORTCUT_PREV]) || DEFAULT_SHORTCUTS.prevWallpaper;

  const volume = Number(volumeStr ?? "0");
  const isMuted = volume === 0;
  const isCloseToTray = closeToTray === "true";
  const isPauseOnFullscreen = pauseOnFullscreen === "true";

  // ===== 音量 =====
  const handleVolumeChange = useCallback(
    (value: number[]) => {
      updateSetting(SETTING_KEYS.GLOBAL_VOLUME, String(value[0]));
    },
    [updateSetting],
  );

  const toggleMute = useCallback(() => {
    updateSetting(SETTING_KEYS.GLOBAL_VOLUME, isMuted ? "50" : "0");
  }, [isMuted, updateSetting]);

  // ===== 托盘 =====
  const toggleCloseToTray = useCallback(
    (checked: boolean) => {
      updateSetting(SETTING_KEYS.CLOSE_TO_TRAY, checked ? "true" : "false");
    },
    [updateSetting],
  );

  const togglePauseOnFullscreen = useCallback(
    (checked: boolean) => {
      updateSetting(SETTING_KEYS.PAUSE_ON_FULLSCREEN, checked ? "true" : "false");
    },
    [updateSetting],
  );

  // ===== 开机自启 =====
  const [autoStartEnabled, setAutoStartEnabled] = useState(false);

  useEffect(() => {
    isEnabled()
      .then(setAutoStartEnabled)
      .catch((e) => console.error("[autostart:isEnabled]", e));
  }, []);

  const toggleAutoStart = useCallback(async (checked: boolean) => {
    try {
      if (checked) {
        await enable();
      } else {
        await disable();
      }
      setAutoStartEnabled(checked);
    } catch (e) {
      console.error("[autostart:toggle]", e);
    }
  }, []);

  return {
    volume,
    isMuted,
    isCloseToTray,
    isPauseOnFullscreen,
    autoStartEnabled,
    shortcutNext,
    shortcutPrev,
    updateSetting,
    handleVolumeChange,
    toggleMute,
    toggleCloseToTray,
    togglePauseOnFullscreen,
    toggleAutoStart,
  };
}
