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
      <DialogContent
        showCloseButton={false}
        className="sm:max-w-160 gap-0 overflow-hidden rounded-xl border-border/40 p-0 fluent-shadow-lg"
      >
        <DialogTitle className="sr-only">{t("settings.title")}</DialogTitle>
        <div className="flex h-[520px]">
          {/* ===== 左侧导航栏 - Win11 Settings 风格 ===== */}
          <nav className="flex w-[180px] shrink-0 flex-col bg-surface/80 py-5">
            <h2 className="mb-4 px-5 text-[13px] font-semibold tracking-wide text-foreground/80 uppercase">
              {t("settings.title")}
            </h2>
            <div className="flex flex-col gap-0.5 px-3">
              {navItems.map(({ id, icon: Icon, labelKey }) => (
                <button
                  key={id}
                  type="button"
                  onClick={() => setActiveSection(id)}
                  className={cn(
                    "relative flex w-full items-center gap-2.5 rounded-md px-3 py-[7px] text-[13px] transition-all duration-100",
                    activeSection === id
                      ? "bg-foreground/7 text-foreground font-medium"
                      : "text-foreground/60 hover:bg-foreground/4 hover:text-foreground/80",
                  )}
                >
                  {/* Win11 左侧指示条 */}
                  <span
                    className={cn(
                      "absolute left-0 top-1/2 h-4 w-[3px] -translate-y-1/2 rounded-full bg-primary transition-all duration-200",
                      activeSection === id ? "opacity-100 scale-y-100" : "opacity-0 scale-y-50",
                    )}
                  />
                  <Icon className="size-[15px] shrink-0" />
                  <span>{t(labelKey)}</span>
                </button>
              ))}
            </div>
          </nav>

          {/* ===== 分隔线 ===== */}
          <div className="w-px bg-border/40" />

          {/* ===== 右侧设置面板 - Win11 卡片式布局 ===== */}
          <div className="flex-1 overflow-y-auto bg-background">
            <div className="px-7 py-6">
              {activeSection === "startup" && (
                <section className="space-y-5">
                  <h3 className="text-[15px] font-semibold text-foreground">
                    {t("settings.navStartup")}
                  </h3>
                  {/* 设置卡片 */}
                  <div className="rounded-lg border border-border/50 bg-card">
                    <div className="flex items-center justify-between px-4 py-3.5">
                      <div className="space-y-0.5">
                        <Label className="text-[13px] font-medium">{t("settings.autoStart")}</Label>
                        <p className="text-[11px] leading-relaxed text-foreground/45">
                          {t("settings.autoStartDesc")}
                        </p>
                      </div>
                      <Switch
                        checked={autoStartEnabled}
                        onCheckedChange={toggleAutoStart}
                      />
                    </div>
                  </div>
                </section>
              )}

              {activeSection === "tray" && (
                <section className="space-y-5">
                  <h3 className="text-[15px] font-semibold text-foreground">
                    {t("settings.navTray")}
                  </h3>
                  {/* 设置卡片 - 多项合并 */}
                  <div className="rounded-lg border border-border/50 bg-card">
                    <div className="flex items-center justify-between px-4 py-3.5">
                      <div className="space-y-0.5">
                        <Label className="text-[13px] font-medium">{t("settings.closeToTray")}</Label>
                        <p className="text-[11px] leading-relaxed text-foreground/45">
                          {t("settings.closeToTrayDesc")}
                        </p>
                      </div>
                      <Switch
                        checked={isCloseToTray}
                        onCheckedChange={toggleCloseToTray}
                      />
                    </div>
                    <div className="mx-4 h-px bg-border/30" />
                    <div className="flex items-center justify-between px-4 py-3.5">
                      <div className="space-y-0.5">
                        <Label className="text-[13px] font-medium">{t("settings.pauseOnFullscreen")}</Label>
                        <p className="text-[11px] leading-relaxed text-foreground/45">
                          {t("settings.pauseOnFullscreenDesc")}
                        </p>
                      </div>
                      <Switch
                        checked={isPauseOnFullscreen}
                        onCheckedChange={togglePauseOnFullscreen}
                      />
                    </div>
                  </div>
                </section>
              )}

              {activeSection === "shortcuts" && (
                <ShortcutsSection
                  shortcutNext={shortcutNext}
                  shortcutPrev={shortcutPrev}
                  updateSetting={updateSetting}
                />
              )}

              {activeSection === "audio" && (
                <section className="space-y-5">
                  <h3 className="text-[15px] font-semibold text-foreground">
                    {t("settings.navAudio")}
                  </h3>
                  {/* 音量卡片 */}
                  <div className="rounded-lg border border-border/50 bg-card">
                    <div className="px-4 py-4 space-y-3">
                      <div className="flex items-center justify-between">
                        <Label className="text-[13px] font-medium">{t("settings.volume")}</Label>
                        <span className="text-[11px] tabular-nums text-foreground/45">
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
                              ? "text-foreground/40 hover:bg-foreground/5 hover:text-foreground/60"
                              : "text-foreground/70 hover:bg-foreground/5 hover:text-foreground",
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
                          className="flex-1"
                        />
                      </div>
                    </div>
                  </div>
                </section>
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
