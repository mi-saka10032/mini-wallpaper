import { useCallback, useEffect, useState, useRef, lazy, Suspense } from "react";
import { useTranslation } from "react-i18next";
import { TooltipProvider } from "@/components/ui/tooltip";
import Toolbar from "@/components/layout/Toolbar";
import Sidebar from "@/components/layout/Sidebar";
import MainContent from "@/components/layout/MainContent";
import { Toaster } from "@/components/ui/toast";
import ErrorBoundary from "@/components/ui/ErrorBoundary";
import { useSettingStore, SETTING_KEYS } from "@/stores/settingStore";
import { useFavoritesStore } from "@/stores/favoritesStore";
import { useTrashedStore } from "@/stores/trashedStore";
import { useWallpaperStore } from "@/stores/wallpaperStore";
import { publish, Signal, usePubSubStore } from "@/stores/pubsubStore";
import { useCollectionStore } from "@/stores/collectionStore";
import { listen, EVENTS } from "@/api/event";
import { useMonitorHotPlug } from "@/hooks/useMonitorHotPlug";
import { useWebGuard } from "@/hooks/useWebGuard";
import { useAccentColor } from "@/hooks/useAccentColor";
import { useStartupUpdateCheck } from "@/hooks/useStartupUpdateCheck";
import { useMonitorConfigStore } from "@/stores/monitorConfigStore";
import { changeLanguage } from "@/i18n";
import { invoke } from "@/api/invoke";
import { COMMANDS } from "@/api/config";
import AppLoading from "@/components/ui/AppLoading";
import { SETTINGS_VIEW_ID, TRASH_VIEW_ID } from "@/constants/views";
import { cn } from "@/lib/utils";

// 非首屏组件懒加载
const PreviewDialog = lazy(() => import("@/components/wallpaper/PreviewDialog"));
const MonitorSettingsPanel = lazy(() => import("@/components/settings/MonitorSettingsPanel"));
const TrashView = lazy(() => import("@/components/wallpaper/TrashView"));

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
  const fetchCollections = useCollectionStore((s) => s.fetchCollections);
  const initFavorites = useFavoritesStore((s) => s.init);

  useEffect(() => {
    const init = async () => {
      try {
        await Promise.all([
          fetchSettings(),
          fetchWallpapers(),
          initMonitors(),
          fetchCollections(),
          invoke(COMMANDS.INIT_FULLSCREEN_DETECTION),
        ]);
        // 依赖上面已就绪的 collections，复用同一次拉取来定位内置收藏夹
        await initFavorites();
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
  }, [fetchSettings, fetchWallpapers, initMonitors, fetchCollections, initFavorites]);

  // 互斥渲染：Loading 退出后才挂载 App，避免 App 加载抢占主线程导致动画卡顿
  if (!loadingExited) {
    return <AppLoading finished={initDone} onExited={() => setLoadingExited(true)} />;
  }

  return <App />;
};

const App: React.FC = () => {
  const { t } = useTranslation();
  useMonitorHotPlug();
  useWebGuard();
  useAccentColor(); // 初始化主题色（启动时应用持久化的 accent color）
  useStartupUpdateCheck(); // 启动后台静默检查更新（最新/失败均无提示）
  const importing = useWallpaperStore((s) => s.loading);
  const language = useSettingStore((s) => s.settings[SETTING_KEYS.LANGUAGE]);
  const applyFavoriteChange = useFavoritesStore((s) => s.applyChange);

  // 失效信号的唯一订阅点：invoke 写命令与后端事件发布到同一个商店。
  // 四类信号各自比对版本号，只重取受影响的那份快照；
  // store 内部不再有任何手动 fetch / refresh，全部由这里统一驱动。
  // subscribe 返回的注销函数交给 effect cleanup，StrictMode / HMR 不重复注册。
  useEffect(() => {
    return usePubSubStore.subscribe((state, prev) => {
      if (state.viewVersion !== prev.viewVersion) {
        void useWallpaperStore
          .getState()
          .sync()
          .catch(() => {});
      }
      if (state.favoritesVersion !== prev.favoritesVersion) {
        void useFavoritesStore
          .getState()
          .refresh()
          .catch(() => {});
      }
      if (state.trashVersion !== prev.trashVersion) {
        void useTrashedStore
          .getState()
          .fetchTrashed()
          .catch(() => {});
      }
      if (state.collectionsVersion !== prev.collectionsVersion) {
        void useCollectionStore
          .getState()
          .fetchCollections()
          .catch(() => {});
      }
    });
  }, []);

  // 后端快捷键 / 托盘触发的收藏变更不经过前端 invoke，需手动发布
  useEffect(() => {
    const unlisten = listen(EVENTS.FAVORITES_CHANGED, ({ wallpaper_id, favorited }) => {
      applyFavoriteChange(wallpaper_id, favorited);
      publish([Signal.VIEW, Signal.FAVORITES]);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [applyFavoriteChange]);

  // activeId / viewWallpapers 由 wallpaperStore 持有：
  // 视图切换与数据拉取在 store 内完成（含竞态守卫），组件只订阅结果。
  const activeId = useWallpaperStore((s) => s.activeId);
  const setActiveId = useWallpaperStore((s) => s.setActiveId);
  const viewWallpapers = useWallpaperStore((s) => s.viewWallpapers);
  const [previewIndex, setPreviewIndex] = useState<number | null>(null);

  // 管理模式状态（用于蒙层遮挡）
  const [manageMode, setManageMode] = useState(false);

  // 回收站首次访问后才挂载（避免未使用时就拉取回收站数据），
  // 挂载后保持常驻，切走仅隐藏，保留选中态与滚动位置。
  const [trashMounted, setTrashMounted] = useState(false);
  useEffect(() => {
    if (activeId === TRASH_VIEW_ID) setTrashMounted(true);
  }, [activeId]);

  const isOverlayView = activeId === SETTINGS_VIEW_ID || activeId === TRASH_VIEW_ID;

  // 跳过首次执行的标记（AppShell 已完成初始化，无需重复）
  const langInitRef = useRef(true);

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

  const openPreview = useCallback((index: number) => {
    setPreviewIndex(index);
  }, []);

  const closePreview = useCallback(() => {
    setPreviewIndex(null);
  }, []);

  return (
    <TooltipProvider>
      <div className="relative h-screen w-screen overflow-hidden rounded-xl bg-background text-foreground fluent-shadow-lg">
        {/* 顶部工具栏 */}
        <div className="relative">
          <Toolbar onActiveIdChange={setActiveId} />
          {/* 管理模式蒙层 - 覆盖 Toolbar */}
          {manageMode && <div className="absolute inset-0 z-40 bg-black/20 backdrop-blur-[1px]" />}
        </div>

        <main className="main-gradient flex h-[calc(100vh-48px)]">
          {/* 侧边栏 */}
          <div className="relative h-full shrink-0">
            <Sidebar activeId={activeId} onActiveIdChange={setActiveId} />
            {/* 管理模式蒙层 - 覆盖 Sidebar */}
            {manageMode && (
              <div className="absolute inset-0 z-40 bg-black/20 backdrop-blur-[1px]" />
            )}
          </div>

          <ErrorBoundary>
            <div className="relative flex flex-1 flex-col overflow-hidden">
              <Suspense fallback={null}>
                <div
                  aria-hidden={activeId !== SETTINGS_VIEW_ID}
                  inert={activeId !== SETTINGS_VIEW_ID}
                  data-state={activeId === SETTINGS_VIEW_ID ? "open" : "closed"}
                  className={cn(
                    "panel-animate absolute inset-0 z-30 overflow-hidden bg-background",
                    activeId !== SETTINGS_VIEW_ID && "pointer-events-none",
                  )}
                >
                  <MonitorSettingsPanel />
                </div>
              </Suspense>

              <Suspense fallback={null}>
                <div
                  aria-hidden={activeId !== TRASH_VIEW_ID}
                  inert={activeId !== TRASH_VIEW_ID}
                  data-state={activeId === TRASH_VIEW_ID ? "open" : "closed"}
                  className={cn(
                    "panel-animate absolute inset-0 z-30 overflow-hidden bg-background",
                    activeId !== TRASH_VIEW_ID && "pointer-events-none",
                  )}
                >
                  {trashMounted && <TrashView />}
                </div>
              </Suspense>

              <div className={cn("flex min-h-0 flex-1 flex-col", isOverlayView && "hidden")}>
                <MainContent onPreview={openPreview} onManageModeChange={setManageMode} />
              </div>
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

        {/* 导入中全局蒙层 */}
        <div
          className={cn(
            "absolute inset-0 z-100 flex items-center justify-center rounded-lg bg-black/40 backdrop-blur-sm",
            { hidden: !importing },
          )}
        >
          <div className="flex flex-col items-center gap-3 rounded-lg bg-popover/95 px-8 py-6 fluent-shadow-lg border border-border/50">
            <div className="size-8 animate-spin rounded-full border-4 border-primary border-t-transparent" />
            <span className="text-sm font-medium text-foreground">{t("main.importing")}</span>
          </div>
        </div>

        {/* 全局 Toast 消息容器 (sonner) */}
        <Toaster position="top-center" richColors closeButton duration={4000} />
      </div>
    </TooltipProvider>
  );
};

export default App;
