import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { isEnabled, enable, disable } from "@tauri-apps/plugin-autostart";
import { save, open as openDialog } from "@tauri-apps/plugin-dialog";
import { listen, EVENTS } from "@/api/event";
import { invoke } from "@/api/invoke";
import { COMMANDS } from "@/api/config";
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

export function useShortcutRecorder(updateSetting: (key: string, value: string) => void) {
  const [recordingAction, setRecordingAction] = useState<string | null>(null);
  const [pendingShortcut, setPendingShortcut] = useState<string | null>(null);
  const recorderRef = useRef<HTMLDivElement>(null);
  const pendingRef = useRef<string | null>(null);
  const recordingRef = useRef<string | null>(null);

  /** 将 KeyboardEvent.code 转为 Tauri 快捷键字符串 */
  const eventToShortcut = useCallback((e: React.KeyboardEvent): string | null => {
    const code = e.code;
    if (["ControlLeft", "ControlRight", "MetaLeft", "MetaRight",
         "AltLeft", "AltRight", "ShiftLeft", "ShiftRight"].includes(code)) return null;
    if (!e.ctrlKey && !e.metaKey && !e.altKey) return null;

    const parts: string[] = [];
    if (e.ctrlKey || e.metaKey) parts.push("CommandOrControl");
    if (e.altKey) parts.push("Alt");
    if (e.shiftKey) parts.push("Shift");

    const codeMap: Record<string, string> = {
      KeyA: "A", KeyB: "B", KeyC: "C", KeyD: "D", KeyE: "E", KeyF: "F",
      KeyG: "G", KeyH: "H", KeyI: "I", KeyJ: "J", KeyK: "K", KeyL: "L",
      KeyM: "M", KeyN: "N", KeyO: "O", KeyP: "P", KeyQ: "Q", KeyR: "R",
      KeyS: "S", KeyT: "T", KeyU: "U", KeyV: "V", KeyW: "W", KeyX: "X",
      KeyY: "Y", KeyZ: "Z",
      Digit0: "0", Digit1: "1", Digit2: "2", Digit3: "3", Digit4: "4",
      Digit5: "5", Digit6: "6", Digit7: "7", Digit8: "8", Digit9: "9",
      F1: "F1", F2: "F2", F3: "F3", F4: "F4", F5: "F5", F6: "F6",
      F7: "F7", F8: "F8", F9: "F9", F10: "F10", F11: "F11", F12: "F12",
      ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right",
      Space: "Space", Escape: "Escape", Enter: "Enter", Backspace: "Backspace",
      Delete: "Delete", Tab: "Tab", Home: "Home", End: "End",
      PageUp: "PageUp", PageDown: "PageDown",
      Minus: "-", Equal: "=", BracketLeft: "[", BracketRight: "]",
      Backslash: "\\", Semicolon: ";", Quote: "'", Comma: ",",
      Period: ".", Slash: "/", Backquote: "`",
    };

    const key = codeMap[code];
    if (!key) return null;
    parts.push(key);

    return parts.join("+");
  }, []);

  const handleRecordKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const shortcut = eventToShortcut(e);
      if (shortcut) {
        pendingRef.current = shortcut;
        setPendingShortcut(shortcut);
      }
    },
    [eventToShortcut],
  );

  const handleRecordKeyUp = useCallback(
    (e: React.KeyboardEvent) => {
      e.preventDefault();
      const pending = pendingRef.current;
      const action = recordingRef.current;
      if (pending && action) {
        updateSetting(action, pending);
        pendingRef.current = null;
        recordingRef.current = null;
        setPendingShortcut(null);
        setRecordingAction(null);
      }
    },
    [updateSetting],
  );

  const startRecording = useCallback((settingKey: string) => {
    recordingRef.current = settingKey;
    pendingRef.current = null;
    setRecordingAction(settingKey);
    setPendingShortcut(null);
    requestAnimationFrame(() => {
      recorderRef.current?.focus();
    });
  }, []);

  const resetShortcut = useCallback(
    (settingKey: string, defaultValue: string) => {
      updateSetting(settingKey, defaultValue);
      setRecordingAction(null);
    },
    [updateSetting],
  );

  const cancelRecording = useCallback(() => {
    recordingRef.current = null;
    pendingRef.current = null;
    setRecordingAction(null);
    setPendingShortcut(null);
  }, []);

  /** 格式化快捷键显示 */
  const formatShortcut = useCallback((shortcut: string) => {
    const isMac = navigator.platform.toUpperCase().includes("MAC");
    return shortcut
      .replace("CommandOrControl", isMac ? "⌘" : "Ctrl")
      .replace("Alt", isMac ? "⌥" : "Alt")
      .replace("Shift", isMac ? "⇧" : "Shift");
  }, []);

  return {
    recordingAction,
    pendingShortcut,
    recorderRef,
    handleRecordKeyDown,
    handleRecordKeyUp,
    startRecording,
    resetShortcut,
    cancelRecording,
    formatShortcut,
  };
}

export function useBackup(activeSection: string) {
  const { t } = useTranslation();
  const [backupBusy, setBackupBusy] = useState(false);
  const [backupMsg, setBackupMsg] = useState<string | null>(null);
  const [dataSize, setDataSize] = useState<number | null>(null);
  const [progress, setProgress] = useState<{ current: number; total: number } | null>(null);

  // 监听 backup-progress 事件
  useEffect(() => {
    const unlisten = listen(EVENTS.BACKUP_PROGRESS, (payload) => {
      setProgress(payload);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // 进入 backup 分组时获取数据大小
  useEffect(() => {
    if (activeSection === "backup") {
      invoke(COMMANDS.GET_DATA_SIZE).then(setDataSize).catch(() => {});
    }
  }, [activeSection]);

  const formatSize = useCallback((bytes: number) => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
  }, []);

  const handleExport = useCallback(async () => {
    const outputPath = await save({
      defaultPath: `mini-wallpaper-backup-${new Date().toISOString().slice(0, 10)}.zip`,
      filters: [{ name: "ZIP", extensions: ["zip"] }],
    });
    if (!outputPath) return;
    setBackupBusy(true);
    setBackupMsg(null);
    setProgress(null);
    try {
      await invoke(COMMANDS.EXPORT_BACKUP, { outputPath });
      setBackupMsg(t("settings.exportSuccess"));
    } catch (e) {
      setBackupMsg(t("settings.exportFailed") + ": " + String(e));
    } finally {
      setBackupBusy(false);
      setProgress(null);
    }
  }, [t]);

  const handleImport = useCallback(async () => {
    const selected = await openDialog({
      multiple: false,
      filters: [{ name: "ZIP", extensions: ["zip"] }],
    });
    if (!selected) return;
    setBackupBusy(true);
    setBackupMsg(null);
    setProgress(null);
    try {
      const count = await invoke(COMMANDS.IMPORT_BACKUP, { zipPath: selected });
      setBackupMsg(t("settings.importSuccess", { count }));
    } catch (e) {
      setBackupMsg(t("settings.importFailed") + ": " + String(e));
    } finally {
      setBackupBusy(false);
      setProgress(null);
    }
  }, [t]);

  return {
    backupBusy,
    backupMsg,
    dataSize,
    progress,
    formatSize,
    handleExport,
    handleImport,
  };
}
