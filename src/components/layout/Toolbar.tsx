import { Monitor, Plus, Sliders } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useWallpaperStore } from "@/stores/wallpaperStore";
import WindowControls from "./WindowControls";
import ThemeToggle from "./ThemeToggle";
import LanguageToggle from "./LanguageToggle";
import AccentColorToggle from "./AccentColorToggle";

interface ToolbarProps {
  onActiveIdChange: (id: number) => void;
  onOpenSettings: () => void;
}

const Toolbar: React.FC<ToolbarProps> = ({ onActiveIdChange, onOpenSettings }) => {
  const { t } = useTranslation();
  const importWallpapers = useWallpaperStore((s) => s.importWallpapers);

  return (
    <div
      data-tauri-drag-region
      className="flex h-12 shrink-0 items-center border-b border-border px-3"
    >
      {/* 左侧操作按钮 */}
      <div className="flex items-center gap-1.5">
        <Button variant="ghost" size="sm" onClick={importWallpapers}>
          <Plus className="size-4" />
          <span>{t("toolbar.import")}</span>
        </Button>
        <Button variant="ghost" size="sm" onClick={() => onActiveIdChange(-1)}>
          <Monitor className="size-4" />
          <span>{t("toolbar.monitor")}</span>
        </Button>
      </div>

      {/* 中间拖拽区域 */}
      <div data-tauri-drag-region className="flex-1" />

      {/* 右侧语言切换 + 主题切换 + 窗口控制 */}
      <div className="flex items-center gap-1">
        <LanguageToggle />
        <ThemeToggle />
        <AccentColorToggle />
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="icon" className="size-8" onClick={onOpenSettings}>
              <Sliders className="size-4" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>{t("sidebar.globalSettings")}</TooltipContent>
        </Tooltip>
        <WindowControls />
      </div>
    </div>
  );
};

export default Toolbar;