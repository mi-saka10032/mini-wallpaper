import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { TooltipProvider } from "@/components/ui/tooltip";
import Toolbar from "@/components/layout/Toolbar";
import Sidebar from "@/components/layout/Sidebar";
import MainContent from "@/components/layout/MainContent";
import MonitorSettingsPanel from "@/components/settings/MonitorSettingsPanel";
import GlobalSettingsDialog from "@/components/settings/GlobalSettingsPanel";
import PreviewDialog from "@/components/wallpaper/PreviewDialog";
import { Toaster } from "@/components/ui/toast";
import { useWallpaperStore } from "@/stores/wallpaperStore";
import { useSettingStore, SETTING_KEYS } from "@/stores/settingStore";
import { useShortcuts } from "@/hooks/useShortcuts";
import { useMonitorHotPlug } from "@/hooks/useMonitorHotPlug";
import { useWebGuard } from "@/hooks/useWebGuard";
import { useAccentColor } from "@/hooks/useAccentColor";
import { useMonitorConfigStore } from "@/stores/monitorConfigStore";
import { changeLanguage } from "@/i18n";
import { invoke } from "@/api/invoke";
import { COMMANDS } from "@/api/config";
import type { Wallpaper } from "@/api/config";
import { getWallpapers as getCollectionWallpapers } from "@/api/collection";

const App: React.FC = () => {
  const { t } = useTranslation();
  useShortcuts();
  useMonitorHotPlug();
  useWebGuard();
  useAccentColor(); // 初始化主题色（启动时应用持久化的 accent color）
  const initMonitors = useMonitorConfigStore((s) => s.init);
  const wallpapers = useWallpaperStore((s) => s.wallpapers);
  const fetchWallpapers = useWallpaperStore((s) => s.fetchWallpapers);
  const importing = useWallpaperStore((s) => s.loading);
  const fetchSettings = useSettingStore((s) => s.fetchSettings);
  const language = useSettingStore((s) => s.settings[SETTING_KEYS.LANGUAGE]);

  // activeId: 0 = 本地壁纸，>0 = 收藏夹 id
  const [activeId, setActiveId] = useState(0);
  const [viewWallpapers, setViewWallpapers] = useState<Wallpaper[]>([]);
  const [previewIndex, setPreviewIndex] = useState<number | null>(null);

  // 全局设置 Dialog
  const [settingsOpen, setSettingsOpen] = useState(false);

  // 管理模式状态（用于蒙层遮挡）
  const [manageMode, setManageMode] = useState(false);

  useEffect(() => {
    fetchSettings();
    fetchWallpapers();
    initMonitors();
    // 初始化全屏检测（读取 DB 设置，按需启动，首次且唯一一次调用）
    invoke(COMMANDS.INIT_FULLSCREEN_DETECTION).catch((e) =>
      console.error("[initFullscreenDetection]", e),
    );
  }, [fetchSettings, fetchWallpapers, initMonitors]);

  // DB 中 language 变化时同步 i18n
  useEffect(() => {
    if (language) {
      changeLanguage(language);
    }
  }, [language]);

  // 切换视图时加载对应壁纸
  useEffect(() => {
    if (activeId === 0) {
      setViewWallpapers(wallpapers);
    } else if (activeId > 0) {
      getCollectionWallpapers(activeId)
        .then(setViewWallpapers)
        .catch((e) => console.error("[getCollectionWallpapers]", e));
    }
  }, [activeId, wallpapers]);

  const openPreview = useCallback((index: number) => {
    setPreviewIndex(index);
  }, []);

  const closePreview = useCallback(() => {
    setPreviewIndex(null);
  }, []);

  // 收藏夹壁纸变更后刷新视图
  const refreshCollectionView = useCallback(() => {
    if (activeId > 0) {
      getCollectionWallpapers(activeId)
        .then(setViewWallpapers)
        .catch((e) => console.error("[refreshCollectionView]", e));
    }
  }, [activeId]);

  return (
    <TooltipProvider>
      <div className="relative h-screen w-screen overflow-hidden rounded-xl border border-border bg-background text-foreground shadow-2xl">
        {/* 顶部工具栏 */}
        <div className="relative">
          <Toolbar onActiveIdChange={setActiveId} />
          {/* 管理模式蒙层 - 覆盖 Toolbar */}
          {manageMode && (
            <div className="absolute inset-0 z-40 rounded-t-xl bg-black/30" />
          )}
        </div>

        <main className="flex h-[calc(100vh-48px)]">
          {/* 侧边栏 */}
          <div className="relative h-full shrink-0">
            <Sidebar activeId={activeId} onActiveIdChange={setActiveId} onOpenSettings={() => setSettingsOpen(true)} />
            {/* 管理模式蒙层 - 覆盖 Sidebar */}
            {manageMode && (
              <div className="absolute inset-0 z-40 bg-black/30" />
            )}
          </div>

          {activeId === -1 ? (
            <div className="flex-1 overflow-hidden">
              <MonitorSettingsPanel />
            </div>
          ) : (
            <MainContent
              activeId={activeId}
              wallpapers={viewWallpapers}
              onPreview={openPreview}
              onCollectionChanged={refreshCollectionView}
              onManageModeChange={setManageMode}
            />
          )}
        </main>

        {previewIndex !== null && (
          <PreviewDialog
            wallpapers={viewWallpapers}
            initialIndex={previewIndex}
            onClose={closePreview}
          />
        )}

        {/* 全局设置 Dialog */}
        <GlobalSettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />

        {/* 导入中全局蒙层 */}
        {importing && (
          <div className="absolute inset-0 z-[100] flex items-center justify-center rounded-xl bg-black/40 backdrop-blur-sm">
            <div className="flex flex-col items-center gap-3 rounded-xl bg-background/90 px-8 py-6 shadow-2xl">
              <div className="size-8 animate-spin rounded-full border-4 border-primary border-t-transparent" />
              <span className="text-sm font-medium text-foreground">{t("main.importing")}</span>
            </div>
          </div>
        )}

        {/* 全局 Toast 消息容器 (sonner) */}
        <Toaster position="top-center" richColors closeButton duration={4000} />
      </div>
    </TooltipProvider>
  );
};

export default App;