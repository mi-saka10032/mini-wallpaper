import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "path";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

const projectRoot = __dirname;

// https://vite.dev/config/
export default defineConfig(async () => ({
  root: path.resolve(projectRoot, "src/pages"),
  publicDir: path.resolve(projectRoot, "public"),
  cacheDir: path.resolve(projectRoot, "node_modules/.vite"),

  plugins: [react(), tailwindcss()],

  resolve: {
    alias: {
      "@": path.resolve(projectRoot, "./src"),
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    fs: {
      allow: [projectRoot],
    },
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },

  build: {
    outDir: path.resolve(projectRoot, "dist"),
    emptyOutDir: true,
    // Tauri 的三种窗口各有独立入口，避免壁纸/Toast 窗口加载主窗口的 UI 框架
    rollupOptions: {
      output: { minify: { compress: { dropConsole: true, dropDebugger: true } } },
      input: {
        main: path.resolve(projectRoot, "src/pages/index.html"),
        wallpaper: path.resolve(projectRoot, "src/pages/wallpaper.html"),
        toast: path.resolve(projectRoot, "src/pages/toast.html"),
      },
    },
    // Tauri WebView 原生支持 modulepreload，无需注入 polyfill
    modulePreload: {
      polyfill: false,
    },
  },
}));
