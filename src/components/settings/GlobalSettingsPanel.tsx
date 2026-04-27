import { useCallback, useEffect, useRef, useState } from "react";
import {
  AppWindow,
  Download,
  Globe,
  HardDrive,
  Keyboard,
  Monitor,
  Moon,
  Palette,
  Power,
  Sun,
  Upload,
  Volume2,
  VolumeX,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { isEnabled, enable, disable } from "@tauri-apps/plugin-autostart";
import { save, open as openDialog } from "@tauri-apps/plugin-dialog";
import { listen, EVENTS } from "@/api/event";
import { invoke } from "@/api/invoke";
import { COMMANDS } from "@/api/config";
import {
  Dialog,
  DialogContent,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Slider } from "@/components/ui/slider";
import { useSettingStore, SETTING_KEYS } from "@/stores/settingStore";
import { DEFAULT_SHORTCUTS } from "@/hooks/useShortcuts";
import { useTheme } from "@/hooks/useTheme";
import { changeLanguage } from "@/i18n";
import { cn } from "@/lib/utils";

/** 设置分组 ID */
type SettingSection = "general" | "startup" | "tray" | "shortcuts" | "audio" | "backup";

interface GlobalSettingsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

const GlobalSettingsDialog: React.FC<GlobalSettingsDialogProps> = ({
  open,
  onOpenChange,
}) => {
  const { t, i18n } = useTranslation();
  const { theme, setTheme } = useTheme();
  const updateSetting = useSettingStore((s) => s.updateSetting);
  const volumeStr = useSettingStore(
    (s) => s.settings[SETTING_KEYS.GLOBAL_VOLUME],
  );
  const closeToTray = useSettingStore(
    (s) => s.settings[SETTING_KEYS.CLOSE_TO_TRAY],
  );

  const [activeSection, setActiveSection] = useState<SettingSection>("general");

  const volume = Number(volumeStr ?? "0");
  const isMuted = volume === 0;
  const isCloseToTray = closeToTray === "true";

  const pauseOnFullscreen = useSettingStore(
    (s) => s.settings[SETTING_KEYS.PAUSE_ON_FULLSCREEN],
  );
  const isPauseOnFullscreen = pauseOnFullscreen === "true";

  const handleThemeChange = useCallback(
    (value: string) => {
      setTheme(value as "light" | "dark" | "system");
    },
    [setTheme],
  );

  const handleLanguageChange = useCallback(
    (value: string) => {
      changeLanguage(value);
      updateSetting(SETTING_KEYS.LANGUAGE, value);
    },
    [updateSetting],
  );

  const handleVolumeChange = useCallback(
    (value: number[]) => {
      updateSetting(SETTING_KEYS.GLOBAL_VOLUME, String(value[0]));
    },
    [updateSetting],
  );

  const toggleMute = useCallback(() => {
    updateSetting(SETTING_KEYS.GLOBAL_VOLUME, isMuted ? "50" : "0");
  }, [isMuted, updateSetting]);

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

  // ===== 开机自启（autostart plugin 自管持久化）=====
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

  // ===== 快捷键配置 =====
  const shortcutNext = useSettingStore(
    (s) => s.settings[SETTING_KEYS.SHORTCUT_NEXT],
  ) || DEFAULT_SHORTCUTS.nextWallpaper;
  const shortcutPrev = useSettingStore(
    (s) => s.settings[SETTING_KEYS.SHORTCUT_PREV],
  ) || DEFAULT_SHORTCUTS.prevWallpaper;

  // 快捷键录制状态
  const [recordingAction, setRecordingAction] = useState<string | null>(null);
  const [pendingShortcut, setPendingShortcut] = useState<string | null>(null);
  const recorderRef = useRef<HTMLDivElement>(null);
  // ref 镜像，避免 keyup handler 闭包捕获过期值
  const pendingRef = useRef<string | null>(null);
  const recordingRef = useRef<string | null>(null);

  /** 将 KeyboardEvent.code 转为 Tauri 快捷键字符串
   *  用 e.code（物理键码）而非 e.key，避免 macOS Alt 组合键产生 ¬ ≥ 等乱码 */
  const eventToShortcut = useCallback((e: React.KeyboardEvent): string | null => {
    const code = e.code;
    // 忽略单独的修饰键
    if (["ControlLeft", "ControlRight", "MetaLeft", "MetaRight",
         "AltLeft", "AltRight", "ShiftLeft", "ShiftRight"].includes(code)) return null;
    // 至少需要一个修饰键
    if (!e.ctrlKey && !e.metaKey && !e.altKey) return null;

    const parts: string[] = [];
    if (e.ctrlKey || e.metaKey) parts.push("CommandOrControl");
    if (e.altKey) parts.push("Alt");
    if (e.shiftKey) parts.push("Shift");

    // code → Tauri 按键名映射
    const codeMap: Record<string, string> = {
      // 字母
      KeyA: "A", KeyB: "B", KeyC: "C", KeyD: "D", KeyE: "E", KeyF: "F",
      KeyG: "G", KeyH: "H", KeyI: "I", KeyJ: "J", KeyK: "K", KeyL: "L",
      KeyM: "M", KeyN: "N", KeyO: "O", KeyP: "P", KeyQ: "Q", KeyR: "R",
      KeyS: "S", KeyT: "T", KeyU: "U", KeyV: "V", KeyW: "W", KeyX: "X",
      KeyY: "Y", KeyZ: "Z",
      // 数字
      Digit0: "0", Digit1: "1", Digit2: "2", Digit3: "3", Digit4: "4",
      Digit5: "5", Digit6: "6", Digit7: "7", Digit8: "8", Digit9: "9",
      // 功能键
      F1: "F1", F2: "F2", F3: "F3", F4: "F4", F5: "F5", F6: "F6",
      F7: "F7", F8: "F8", F9: "F9", F10: "F10", F11: "F11", F12: "F12",
      // 方向键
      ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right",
      // 特殊键
      Space: "Space", Escape: "Escape", Enter: "Enter", Backspace: "Backspace",
      Delete: "Delete", Tab: "Tab", Home: "Home", End: "End",
      PageUp: "PageUp", PageDown: "PageDown",
      // 符号
      Minus: "-", Equal: "=", BracketLeft: "[", BracketRight: "]",
      Backslash: "\\", Semicolon: ";", Quote: "'", Comma: ",",
      Period: ".", Slash: "/", Backquote: "`",
    };

    const key = codeMap[code];
    if (!key) return null; // 无法识别的键忽略
    parts.push(key);

    return parts.join("+");
  }, []);

  /** keydown: 记录组合键，实时显示 */
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

  /** keyup: 从 ref 读 pending 值，保存并退出录制 */
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

  /** click 激活录制 */
  const startRecording = useCallback((settingKey: string) => {
    recordingRef.current = settingKey;
    pendingRef.current = null;
    setRecordingAction(settingKey);
    setPendingShortcut(null);
    // 下一帧 focus 到录制区域
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

  /** 格式化快捷键显示（将 CommandOrControl 替换为 ⌘/Ctrl） */
  const formatShortcut = useCallback((shortcut: string) => {
    const isMac = navigator.platform.toUpperCase().includes("MAC");
    return shortcut
      .replace("CommandOrControl", isMac ? "⌘" : "Ctrl")
      .replace("Alt", isMac ? "⌥" : "Alt")
      .replace("Shift", isMac ? "⇧" : "Shift");
  }, []);

  // ===== 备份/导出 =====
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

  /** 侧边栏导航项 */
  const navItems: { id: SettingSection; icon: React.ElementType; labelKey: string }[] = [
    { id: "general", icon: Palette, labelKey: "settings.navGeneral" },
    { id: "startup", icon: Power, labelKey: "settings.navStartup" },
    { id: "tray", icon: AppWindow, labelKey: "settings.navTray" },
    { id: "shortcuts", icon: Keyboard, labelKey: "settings.navShortcuts" },
    { id: "audio", icon: Volume2, labelKey: "settings.navAudio" },
    { id: "backup", icon: HardDrive, labelKey: "settings.navBackup" },
  ];

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl gap-0 overflow-hidden p-0 sm:max-h-[520px]">
        <DialogTitle className="sr-only">{t("settings.title")}</DialogTitle>
        <div className="flex h-[520px]">
          {/* ===== 左侧导航栏 ===== */}
          <nav className="flex w-44 shrink-0 flex-col border-r border-border bg-muted/30 px-2 py-4">
            <h2 className="mb-3 px-2 text-sm font-semibold text-foreground">
              {t("settings.title")}
            </h2>
            <div className="space-y-0.5">
              {navItems.map(({ id, icon: Icon, labelKey }) => (
                <button
                  key={id}
                  type="button"
                  onClick={() => setActiveSection(id)}
                  className={cn(
                    "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm transition-colors",
                    activeSection === id
                      ? "bg-accent text-accent-foreground"
                      : "text-muted-foreground hover:bg-accent/50 hover:text-foreground",
                  )}
                >
                  <Icon className="size-4" />
                  <span>{t(labelKey)}</span>
                </button>
              ))}
            </div>
          </nav>

          {/* ===== 右侧设置面板 ===== */}
          <ScrollArea className="flex-1">
            <div className="px-6 py-5">
              {activeSection === "general" && (
                <div className="space-y-6">
                  <h3 className="text-base font-semibold">{t("settings.navGeneral")}</h3>

                  {/* 主题 */}
                  <div className="space-y-3">
                    <Label className="text-sm font-medium">{t("settings.theme")}</Label>
                    <div className="flex gap-2">
                      <Button
                        variant={theme === "light" ? "default" : "outline"}
                        size="sm"
                        onClick={() => handleThemeChange("light")}
                      >
                        <Sun className="mr-1.5 size-3.5" />
                        {t("settings.themeLight")}
                      </Button>
                      <Button
                        variant={theme === "dark" ? "default" : "outline"}
                        size="sm"
                        onClick={() => handleThemeChange("dark")}
                      >
                        <Moon className="mr-1.5 size-3.5" />
                        {t("settings.themeDark")}
                      </Button>
                      <Button
                        variant={theme === "system" ? "default" : "outline"}
                        size="sm"
                        onClick={() => handleThemeChange("system")}
                      >
                        <Monitor className="mr-1.5 size-3.5" />
                        {t("settings.themeSystem")}
                      </Button>
                    </div>
                  </div>

                  <Separator />

                  {/* 语言 */}
                  <div className="space-y-3">
                    <Label className="text-sm font-medium">{t("settings.language")}</Label>
                    <Select value={i18n.language} onValueChange={handleLanguageChange}>
                      <SelectTrigger className="w-full max-w-xs">
                        <Globe className="mr-2 size-4 text-muted-foreground" />
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="zh">{t("language.zh")}</SelectItem>
                        <SelectItem value="en">{t("language.en")}</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                </div>
              )}

              {activeSection === "startup" && (
                <div className="space-y-6">
                  <h3 className="text-base font-semibold">{t("settings.navStartup")}</h3>

                  {/* 开机自启 */}
                  <div className="flex items-center justify-between">
                    <div className="space-y-1">
                      <Label className="text-sm font-medium">{t("settings.autoStart")}</Label>
                      <p className="text-xs text-muted-foreground">
                        {t("settings.autoStartDesc")}
                      </p>
                    </div>
                    <Switch
                      checked={autoStartEnabled}
                      onCheckedChange={toggleAutoStart}
                    />
                  </div>
                </div>
              )}

              {activeSection === "tray" && (
                <div className="space-y-6">
                  <h3 className="text-base font-semibold">{t("settings.navTray")}</h3>

                  {/* 关闭时最小化到托盘 */}
                  <div className="flex items-center justify-between">
                    <div className="space-y-1">
                      <Label className="text-sm font-medium">{t("settings.closeToTray")}</Label>
                      <p className="text-xs text-muted-foreground">
                        {t("settings.closeToTrayDesc")}
                      </p>
                    </div>
                    <Switch
                      checked={isCloseToTray}
                      onCheckedChange={toggleCloseToTray}
                    />
                  </div>

                  <Separator />

                  {/* 全屏应用检测自动暂停 */}
                  <div className="flex items-center justify-between">
                    <div className="space-y-1">
                      <Label className="text-sm font-medium">{t("settings.pauseOnFullscreen")}</Label>
                      <p className="text-xs text-muted-foreground">
                        {t("settings.pauseOnFullscreenDesc")}
                      </p>
                    </div>
                    <Switch
                      checked={isPauseOnFullscreen}
                      onCheckedChange={togglePauseOnFullscreen}
                    />
                  </div>
                </div>
              )}

              {activeSection === "shortcuts" && (
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
                          onBlur={() => { recordingRef.current = null; pendingRef.current = null; setRecordingAction(null); setPendingShortcut(null); }}
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
                          onBlur={() => { recordingRef.current = null; pendingRef.current = null; setRecordingAction(null); setPendingShortcut(null); }}
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
              )}

              {activeSection === "audio" && (
                <div className="space-y-6">
                  <h3 className="text-base font-semibold">{t("settings.navAudio")}</h3>

                  {/* 壁纸音量 */}
                  <div className="space-y-3">
                    <div className="flex items-center justify-between">
                      <Label className="text-sm font-medium">{t("settings.volume")}</Label>
                      <span className="text-xs text-muted-foreground">
                        {isMuted ? t("settings.volumeMuted") : `${volume}%`}
                      </span>
                    </div>
                    <div className="flex items-center gap-3">
                      <button
                        type="button"
                        onClick={toggleMute}
                        className={cn(
                          "rounded-md p-1.5 transition-colors",
                          isMuted
                            ? "text-muted-foreground hover:bg-muted"
                            : "text-foreground hover:bg-muted",
                        )}
                      >
                        {isMuted ? (
                          <VolumeX className="size-4" />
                        ) : (
                          <Volume2 className="size-4" />
                        )}
                      </button>
                      <Slider
                        value={[volume]}
                        onValueChange={handleVolumeChange}
                        min={0}
                        max={100}
                        step={1}
                        className="max-w-xs flex-1"
                      />
                    </div>
                  </div>
                </div>
              )}

              {activeSection === "backup" && (
                <div className="space-y-6">
                  <h3 className="text-base font-semibold">{t("settings.navBackup")}</h3>

                  {/* 数据大小 */}
                  {dataSize !== null && (
                    <div className="rounded-md bg-muted/50 px-4 py-3">
                      <div className="flex items-center justify-between">
                        <span className="text-sm text-muted-foreground">{t("settings.dataSize")}</span>
                        <span className="text-sm font-medium">{formatSize(dataSize)}</span>
                      </div>
                    </div>
                  )}

                  <p className="text-xs text-muted-foreground">
                    {t("settings.backupDesc")}
                  </p>

                  {/* 导出 */}
                  <div className="flex items-center gap-3">
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={backupBusy}
                      onClick={handleExport}
                      className="gap-1.5"
                    >
                      <Upload className="size-3.5" />
                      {t("settings.export")}
                    </Button>

                    {/* 导入 */}
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={backupBusy}
                      onClick={handleImport}
                      className="gap-1.5"
                    >
                      <Download className="size-3.5" />
                      {t("settings.import")}
                    </Button>
                  </div>

                  {/* 进度条 */}
                  {backupBusy && progress && progress.total > 0 && (
                    <div className="space-y-1.5">
                      <div className="flex items-center justify-between text-xs text-muted-foreground">
                        <span>{progress.current} / {progress.total}</span>
                        <span>{Math.round((progress.current / progress.total) * 100)}%</span>
                      </div>
                      <div className="h-2 w-full overflow-hidden rounded-full bg-muted">
                        <div
                          className="h-full rounded-full bg-primary transition-all duration-200"
                          style={{ width: `${(progress.current / progress.total) * 100}%` }}
                        />
                      </div>
                    </div>
                  )}

                  {/* 状态消息 */}
                  {backupMsg && (
                    <p className="text-xs text-muted-foreground">{backupMsg}</p>
                  )}
                </div>
              )}
            </div>
          </ScrollArea>
        </div>
      </DialogContent>
    </Dialog>
  );
};

export default GlobalSettingsDialog;
