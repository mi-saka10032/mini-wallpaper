import { createBrowserRouter } from "react-router-dom";
import { AppShell } from "@/App";
import WallpaperRenderer from "@/WallpaperRenderer";

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
    element: <WallpaperRenderer />,
  },
]);

export default router;
