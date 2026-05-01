import { useState } from "react";
import {
  AppWindow,
  HardDrive,
  Keyboard,
  Power,
  Volume2,
  VolumeX,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  Dialog,
  DialogContent,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import { Slider } from "@/components/ui/slider";
import { cn } from "@/lib/utils";
import { useGlobalSettings } from "@/hooks/useGlobalSettings";
import { useBackup } from "@/hooks/useBackup";
import ShortcutsSection from "./ShortcutsSection";
import BackupSection from "./BackupSection";

/** 设置分组 ID */
type SettingSection = "startup" | "tray" | "shortcuts" | "audio" | "backup";

interface GlobalSettingsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

const GlobalSettingsDialog: React.FC<GlobalSettingsDialogProps> = ({
  open,
  onOpenChange,
}) => {
  const { t } = useTranslation();
  const [activeSection, setActiveSection] = useState<SettingSection>("startup");

  const {
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
  } = useGlobalSettings();

  const {
    backupBusy,
    backupMsg,
    dataSize,
    progress,
    formatSize,
    handleExport,
    handleImport,
  } = useBackup(activeSection);

  /** 侧边栏导航项 */
  const navItems: { id: SettingSection; icon: React.ElementType; labelKey: string }[] = [
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
          <div className="flex-1 overflow-y-auto">
            <div className="px-6 py-5">
              {activeSection === "startup" && (
                <div className="space-y-6">
                  <h3 className="text-base font-semibold">{t("settings.navStartup")}</h3>
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
                <ShortcutsSection
                  shortcutNext={shortcutNext}
                  shortcutPrev={shortcutPrev}
                  updateSetting={updateSetting}
                />
              )}

              {activeSection === "audio" && (
                <div className="space-y-6">
                  <h3 className="text-base font-semibold">{t("settings.navAudio")}</h3>
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
                <BackupSection
                  backupBusy={backupBusy}
                  backupMsg={backupMsg}
                  dataSize={dataSize}
                  progress={progress}
                  formatSize={formatSize}
                  onExport={handleExport}
                  onImport={handleImport}
                />
              )}
            </div>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
};

export default GlobalSettingsDialog;
