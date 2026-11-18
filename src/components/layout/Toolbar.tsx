import { lazy, Suspense, useCallback } from "react";
import { Monitor, Plus, Settings } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Dialog, DialogTrigger } from "@/components/ui/dialog";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useImportFeedback } from "@/hooks/useImportFeedback";
import { useWallpaperStore } from "@/stores/wallpaperStore";
import WindowControls from "./WindowControls";
import ThemeToggle from "./ThemeToggle";
import LanguageToggle from "./LanguageToggle";
import AccentColorToggle from "./AccentColorToggle";
import UpdateToast from "./UpdateToast";

const GlobalSettingsDialog = lazy(() => import("@/components/settings/GlobalSettingsPanel"));

interface ToolbarProps {
  onActiveIdChange: (id: number) => void;
}

const Toolbar: React.FC<ToolbarProps> = ({ onActiveIdChange }) => {
  const { t } = useTranslation();
  const importWallpapers = useWallpaperStore((s) => s.importWallpapers);
  const { notify, notifyError } = useImportFeedback();

  // 文件选择器导入。与拖拽两条通道共用同一套提示：
  // 成功弹数量（批量时这是唯一能知道导入了几个的地方），取消则静默。
  const handleImport = useCallback(async () => {
    try {
      notify(await importWallpapers());
    } catch (e) {
      notifyError(e);
    }
  }, [importWallpapers, notify, notifyError]);

  return (
    <div
      data-tauri-drag-region
      className="toolbar-primary flex h-12 shrink-0 items-center border-b border-primary-foreground/15 bg-primary px-3"
    >
      {/* 左侧操作按钮 */}
      <div className="flex items-center gap-1">
        <Button
          variant="ghost"
          size="sm"
          onClick={handleImport}
          className="gap-1.5 text-primary-foreground/70 hover:text-primary-foreground hover:bg-transparent"
        >
          <Plus className="size-4" />
          <span className="text-sm">{t("toolbar.import")}</span>
        </Button>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => onActiveIdChange(-1)}
          className="gap-1.5 text-primary-foreground/70 hover:text-primary-foreground hover:bg-transparent"
        >
          <Monitor className="size-4" />
          <span className="text-sm">{t("toolbar.monitor")}</span>
        </Button>
      </div>

      {/* 中间拖拽区域 */}
      <div data-tauri-drag-region className="flex-1" />

      {/* 右侧语言切换 + 主题切换 + 窗口控制 */}
      <div className="relative flex items-center gap-0.5">
        {/* 新版本提示浮窗：锚定在设置图标下方 */}
        <UpdateToast />
        <LanguageToggle />
        <ThemeToggle />
        <AccentColorToggle />
        <Dialog>
          <Tooltip>
            <TooltipTrigger asChild>
              <DialogTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-8 text-primary-foreground/70 hover:text-primary-foreground hover:bg-transparent"
                >
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
