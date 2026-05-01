import { useCallback, useEffect, useState, useRef, lazy, Suspense } from "react";
import { useTranslation } from "react-i18next";
import { TooltipProvider } from "@/components/ui/tooltip";
import Toolbar from "@/components/layout/Toolbar";
import Sidebar from "@/components/layout/Sidebar";
import MainContent from "@/components/layout/MainContent";
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
const MonitorSettingsPanel = lazy(() => import("@/components/settings/MonitorSettingsPanel"));

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

  // 互斥渲染：Loading 退出后才挂载 App，避免 App 加载抢占主线程导致动画卡顿
  if (!loadingExited) {
    return <AppLoading finished={initDone} onExited={() => setLoadingExited(true)} />;
  }

  return <App />;
};

const App: React.FC = () => {
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

  // 跳过首次执行的标记（AppShell 已完成初始化，无需重复）
  const langInitRef = useRef(true);
  const wpInitRef = useRef(true);

  // DB 中 language 变化时同步 i18n（跳过首次：AppShell 已同步）
  useEffect(() => {
    if (langInitRef.current) {
      langInitRef.current = false;
      return;
    }
    if (language) {
      changeLanguage(language);
    }
  }, [language]);

  // viewWallpapers 数据源切换：本地壁纸 or 收藏夹
  // 跳过首次：useState 初始值已从 store 取得，无需重复 set
  useEffect(() => {
    if (wpInitRef.current) {
      wpInitRef.current = false;
      return;
    }
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
      <div className="relative h-screen w-screen overflow-hidden rounded-lg border border-border/60 bg-background text-foreground fluent-shadow-lg">
        {/* 顶部工具栏 */}
        <div className="relative">
          <Toolbar onActiveIdChange={setActiveId} onOpenSettings={() => setSettingsOpen(true)} />
          {/* 管理模式蒙层 - 覆盖 Toolbar */}
          {manageMode && <div className="absolute inset-0 z-40 bg-black/20 backdrop-blur-[1px]" />}
        </div>

        <main className="flex h-[calc(100vh-48px)]">
          {/* 侧边栏 */}
          <div className="relative h-full shrink-0">
            <Sidebar activeId={activeId} onActiveIdChange={setActiveId} />
            {/* 管理模式蒙层 - 覆盖 Sidebar */}
            {manageMode && <div className="absolute inset-0 z-40 bg-black/20 backdrop-blur-[1px]" />}
          </div>

          <ErrorBoundary>
            <div className="relative flex-1 overflow-hidden">
              {/* 显示器设置面板 - 懒加载覆盖层，不影响首屏加载 */}
              <Suspense fallback={null}>
                <div
                  className={cn(
                    "absolute inset-0 z-30 overflow-hidden bg-background",
                    activeId === -1 ? "block" : "hidden",
                  )}
                >
                  <MonitorSettingsPanel />
                </div>
              </Suspense>

              <MainContent
                activeId={activeId}
                wallpapers={viewWallpapers}
                onPreview={openPreview}
                onCollectionChanged={refreshCollectionView}
                onManageModeChange={setManageMode}
              />
            </div>
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
          <div className="absolute inset-0 z-[100] flex items-center justify-center rounded-lg bg-black/40 backdrop-blur-sm">
            <div className="flex flex-col items-center gap-3 rounded-lg bg-popover/95 px-8 py-6 fluent-shadow-lg border border-border/50">
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