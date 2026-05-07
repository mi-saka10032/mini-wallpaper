import { createBrowserRouter } from "react-router-dom";
import { lazy, Suspense } from "react";
import { AppShell } from "@/App";

const WallpaperRenderer = lazy(() => import("@/WallpaperRenderer"));

/**
 * 路由表
 * - /           主窗口（设置管理界面）
 * - /wallpaper  壁纸窗口（Rust 通过 WebviewWindow 打开，每个显示器一个）
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
]);

export default router;
