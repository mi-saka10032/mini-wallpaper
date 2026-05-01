import { useCallback, useEffect, useState, lazy, Suspense } from "react";
import { useTranslation } from "react-i18next";
import { TooltipProvider } from "@/components/ui/tooltip";
import Toolbar from "@/components/layout/Toolbar";
import Sidebar from "@/components/layout/Sidebar";
import MainContent from "@/components/layout/MainContent";
import MonitorSettingsPanel from "@/components/settings/MonitorSettingsPanel";
import { Toaster } from "@/components/ui/toast";
import ErrorBoundary from "@/components/ui/ErrorBoundary";
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
import { getCollectionWallpapers } from "@/api/collection";
import AppLoading from "@/components/ui/AppLoading";
import { cn } from "./lib/utils";

// 非首屏组件懒加载
const GlobalSettingsDialog = lazy(() => import("@/components/settings/GlobalSettingsPanel"));
const PreviewDialog = lazy(() => import("@/components/wallpaper/PreviewDialog"));

/**
 * AppShell - 外层容器，负责初始化逻辑
 * 初始化完成前只渲染 Loading，完成后才挂载 App 主体
 */
export const AppShell: React.FC = () => {
  const [initDone, setInitDone] = useState(false);
  const [loadingExited, setLoadingExited] = useState(false);

  const fetchSettings = useSettingStore((s) => s.fetchSettings);
  const fetchWallpapers = useWallpaperStore((s) => s.fetchWallpapers);
  const initMonitors = useMonitorConfigStore((s) => s.init);

  useEffect(() => {
    const init = async () => {
      try {
        await Promise.all([
          fetchSettings(),
          fetchWallpapers(),
          initMonitors(),
          invoke(COMMANDS.INIT_FULLSCREEN_DETECTION).catch((e) =>
            console.error("[initFullscreenDetection]", e),
          ),
        ]);
        // 初始化完成后立即同步语言，避免 App 挂载时出现语言闪烁
        const lang = useSettingStore.getState().settings[SETTING_KEYS.LANGUAGE];
        if (lang) changeLanguage(lang);
      } catch (e) {
        console.error("[App init]", e);
      } finally {
        setInitDone(true);
      }
    };
    init();
  }, [fetchSettings, fetchWallpapers, initMonitors]);

  return (
    <>
      {/* Loading 与 App 同时挂载，通过显隐切换避免白屏 */}
      {!loadingExited && <AppLoading finished={initDone} onExited={() => setLoadingExited(true)} />}
      <div
        className={`transition-opacity duration-300 ${
          loadingExited ? "opacity-100" : "invisible opacity-0"
        }`}
      >
        <App hideBorder={!loadingExited} />
      </div>
    </>
  );
};

const App: React.FC<{ hideBorder?: boolean }> = ({ hideBorder }) => {
  const { t } = useTranslation();
  useShortcuts();
  useMonitorHotPlug();
  useWebGuard();
  useAccentColor(); // 初始化主题色（启动时应用持久化的 accent color）
  const wallpapers = useWallpaperStore((s) => s.wallpapers);
  const importing = useWallpaperStore((s) => s.loading);
  const language = useSettingStore((s) => s.settings[SETTING_KEYS.LANGUAGE]);

  // activeId: 0 = 本地壁纸，>0 = 收藏夹 id
  const [activeId, setActiveId] = useState(0);
  // 初始值直接从 store 取，避免首帧空列表闪烁（AppShell 已确保数据就绪）
  const [viewWallpapers, setViewWallpapers] = useState<Wallpaper[]>(wallpapers);
  const [previewIndex, setPreviewIndex] = useState<number | null>(null);

  // 全局设置 Dialog
  const [settingsOpen, setSettingsOpen] = useState(false);

  // 管理模式状态（用于蒙层遮挡）
  const [manageMode, setManageMode] = useState(false);

  // DB 中 language 变化时同步 i18n
  useEffect(() => {
    if (language) {
      changeLanguage(language);
    }
  }, [language]);

  // viewWallpapers 数据源切换：本地壁纸 or 收藏夹
  // 拆分为两个 effect，避免收藏夹视图下 wallpapers 变化触发无意义的网络请求
  useEffect(() => {
    if (activeId === 0) {
      setViewWallpapers(wallpapers);
    }
  }, [activeId, wallpapers]);

  useEffect(() => {
    if (activeId > 0) {
      getCollectionWallpapers(activeId)
        .then(setViewWallpapers)
        .catch((e) => console.error("[getCollectionWallpapers]", e));
    }
  }, [activeId]);

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
      <div
        className={`relative h-screen w-screen overflow-hidden rounded-xl ${
          hideBorder ? "" : "border border-border"
        } bg-background text-foreground shadow-2xl`}
      >
        {/* 顶部工具栏 */}
        <div className="relative">
          <Toolbar onActiveIdChange={setActiveId} onOpenSettings={() => setSettingsOpen(true)} />
          {/* 管理模式蒙层 - 覆盖 Toolbar */}
          {manageMode && <div className="absolute inset-0 z-40 rounded-t-xl bg-black/30" />}
        </div>

        <main className="flex h-[calc(100vh-48px)]">
          {/* 侧边栏 */}
          <div className="relative h-full shrink-0">
            <Sidebar activeId={activeId} onActiveIdChange={setActiveId} />
            {/* 管理模式蒙层 - 覆盖 Sidebar */}
            {manageMode && <div className="absolute inset-0 z-40 bg-black/30" />}
          </div>

          <ErrorBoundary>
            <div className={cn("flex-1 overflow-hidden", activeId === -1 ? "block" : "hidden")}>
              <MonitorSettingsPanel />
            </div>
            <MainContent
              className={activeId !== -1 ? "block" : "hidden"}
              activeId={activeId}
              wallpapers={viewWallpapers}
              onPreview={openPreview}
              onCollectionChanged={refreshCollectionView}
              onManageModeChange={setManageMode}
            />
          </ErrorBoundary>
        </main>

        {previewIndex !== null && (
          <ErrorBoundary>
            <Suspense fallback={null}>
              <PreviewDialog
                wallpapers={viewWallpapers}
                initialIndex={previewIndex}
                onClose={closePreview}
              />
            </Suspense>
          </ErrorBoundary>
        )}

        {/* 全局设置 Dialog */}
        <ErrorBoundary>
          <Suspense fallback={null}>
            <GlobalSettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />
          </Suspense>
        </ErrorBoundary>

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
