import { createBrowserRouter } from "react-router-dom";
import App from "@/App";
import WallpaperRenderer from "@/WallpaperRenderer";

/**
 * 路由表
 * - /           主窗口（设置管理界面）
 * - /wallpaper  壁纸窗口（Rust 通过 WebviewWindow 打开，每个显示器一个）
 */
const router = createBrowserRouter([
  {
    path: "/",
    element: <App />,
  },
  {
    path: "/wallpaper",
    element: <WallpaperRenderer />,
  },
]);

export default router;
