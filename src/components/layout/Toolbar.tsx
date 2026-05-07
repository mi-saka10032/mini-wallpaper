import { lazy, Suspense } from "react";
import { Monitor, Plus, Settings } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Dialog, DialogTrigger } from "@/components/ui/dialog";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useWallpaperStore } from "@/stores/wallpaperStore";
import WindowControls from "./WindowControls";
import ThemeToggle from "./ThemeToggle";
import LanguageToggle from "./LanguageToggle";
import AccentColorToggle from "./AccentColorToggle";

const GlobalSettingsDialog = lazy(() => import("@/components/settings/GlobalSettingsPanel"));

interface ToolbarProps {
  onActiveIdChange: (id: number) => void;
}

const Toolbar: React.FC<ToolbarProps> = ({ onActiveIdChange }) => {
  const { t } = useTranslation();
  const importWallpapers = useWallpaperStore((s) => s.importWallpapers);

  return (
    <div
      data-tauri-drag-region
      className="flex h-12 shrink-0 items-center border-b border-border/50 bg-surface px-3"
    >
      {/* 左侧操作按钮 */}
      <div className="flex items-center gap-1">
        <Button variant="ghost" size="sm" onClick={importWallpapers} className="gap-1.5 text-foreground/70 hover:text-foreground hover:bg-primary-hover">
          <Plus className="size-4" />
          <span className="text-[13px]">{t("toolbar.import")}</span>
        </Button>
        <Button variant="ghost" size="sm" onClick={() => onActiveIdChange(-1)} className="gap-1.5 text-foreground/70 hover:text-foreground hover:bg-primary-hover">
          <Monitor className="size-4" />
          <span className="text-[13px]">{t("toolbar.monitor")}</span>
        </Button>
      </div>

      {/* 中间拖拽区域 */}
      <div data-tauri-drag-region className="flex-1" />

      {/* 右侧语言切换 + 主题切换 + 窗口控制 */}
      <div className="flex items-center gap-0.5">
        <LanguageToggle />
        <ThemeToggle />
        <AccentColorToggle />
        <Dialog>
          <Tooltip>
            <TooltipTrigger asChild>
              <DialogTrigger asChild>
                <Button variant="ghost" size="icon" className="size-8 text-foreground/70 hover:text-foreground hover:bg-primary-hover">
                  <Settings className="size-4" />
                </Button>
              </DialogTrigger>
            </TooltipTrigger>
            <TooltipContent>{t("sidebar.globalSettings")}</TooltipContent>
          </Tooltip>
          <Suspense fallback={null}>
            <GlobalSettingsDialog />
          </Suspense>
        </Dialog>
        <WindowControls />
      </div>
    </div>
  );
};

export default Toolbar;
