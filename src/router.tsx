import { createBrowserRouter } from "react-router-dom";
import { lazy, Suspense } from "react";
import { AppShell } from "@/App";

const WallpaperRenderer = lazy(() => import("@/WallpaperRenderer"));
const ActionToast = lazy(() => import("@/ActionToast"));

/**
 * 路由表
 * - /           主窗口（设置管理界面）
 * - /wallpaper  壁纸窗口（Rust 通过 WebviewWindow 打开，每个显示器一个）
 * - /toast      Toast 通知窗口（Rust 通过 toast_manager 创建，右下角独立窗口）
 */
const router = createBrowserRouter([
  {
    path: "/",
    element: <AppShell />,
  },
  {
    path: "/wallpaper",
    element: (
      <Suspense fallback={null}>
        <WallpaperRenderer />
      </Suspense>
    ),
  },
  {
    path: "/toast",
    element: (
      <Suspense fallback={null}>
        <ActionToast />
      </Suspense>
    ),
  },
]);

export default router;