import { useEffect, useRef } from "react";
import { useUpdaterStore } from "@/stores/updaterStore";

/** 启动后延迟检查的毫秒数：让出首屏渲染主线程，避免与初始化数据加载抢资源 */
const STARTUP_CHECK_DELAY = 3000;

/**
 * 应用启动时的静默更新检查
 *
 * 行为约定（对应需求 1）：
 * - 进入应用后在后台静默检查，不阻塞首屏
 * - 已是最新版本 → 无任何提示
 * - 检查失败（离线 / GitHub 不可达）→ 无任何提示，仅留日志
 * - 发现新版本 → 由 store 置 toastVisible，右上角浮窗自行展示
 *
 * dev 模式下 updater 无签名产物可用，check 必然失败，故直接跳过。
 */
export function useStartupUpdateCheck(): void {
  const initVersion = useUpdaterStore((s) => s.initVersion);
  const runCheck = useUpdaterStore((s) => s.runCheck);
  // StrictMode 下 effect 会执行两次，用 ref 保证检查只发起一次
  const startedRef = useRef(false);

  useEffect(() => {
    if (startedRef.current) return;
    startedRef.current = true;

    // 版本号始终读取，设置面板需要展示
    initVersion();

    if (import.meta.env.DEV) return;

    const timer = window.setTimeout(() => {
      runCheck(true);
    }, STARTUP_CHECK_DELAY);

    return () => window.clearTimeout(timer);
  }, [initVersion, runCheck]);
}
