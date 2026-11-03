import { useMemo } from "react";
import { useMonitorConfigStore } from "@/stores/monitorConfigStore";

/**
 * 反查某张壁纸当前正被哪些 active 显示器使用。
 *
 * 设计约束：zustand 的选择器若每次返回新数组/新对象，会因引用不等而不断触发重渲染。
 * 因此这里让选择器只返回一个稳定的字符串（monitor_id 以 "\u0000" 连接），
 * 再在组件侧拆回数组，保证「壁纸未被使用」或「使用关系未变化」时组件不重渲染。
 *
 * 天然覆盖单显示器场景：只有一台显示器时返回该显示器一项，而非退化为无显示器信息。
 */
const IN_USE_SEP = "\u0000";

export function useMonitorsUsingWallpaper(wallpaperId: number): string[] {
  const joined = useMonitorConfigStore((s) => {
    let acc = "";
    for (const c of s.configs) {
      if (c.active && c.wallpaper_id === wallpaperId) {
        acc = acc === "" ? c.monitor_id : acc + IN_USE_SEP + c.monitor_id;
      }
    }
    return acc;
  });

  return useMemo(() => (joined === "" ? [] : joined.split(IN_USE_SEP)), [joined]);
}
