import { availableMonitors, type Monitor } from "@tauri-apps/api/window";
import type { MonitorConfig } from "@/api/config";
import {
  getMonitorConfigs,
  upsertMonitorConfig,
} from "@/api/monitorConfig";
import {
  createWallpaperWindow,
  destroyWallpaperWindow,
  getActiveWallpaperWindows,
} from "@/api/wallpaperWindow";

/** 已创建壁纸窗口的 monitor_id 集合（前端侧跟踪，防止重复创建） */
export const activeWallpaperWindows = new Set<string>();

/**
 * 为 active 且有 wallpaper_id 的显示器创建壁纸窗口
 * display_mode 从全局 app_setting 读取：
 * - independent: 每个显示器独立壁纸
 * - mirror: 所有显示器使用同一壁纸（数据由 Rust 保证一致，窗口正常创建）
 * - extend: 一张壁纸横跨所有显示器，每个窗口通过 availableMonitors() 自行计算裁剪区域
 */
export async function syncWallpaperWindows(
  configs: MonitorConfig[],
  monitors: Monitor[],
) {
  // 先从 Rust 端查询已有壁纸窗口，恢复前端跟踪集合（解决页面刷新后状态丢失问题）
  try {
    const existingWindows = await getActiveWallpaperWindows();
    for (const mid of existingWindows) {
      activeWallpaperWindows.add(mid);
    }
  } catch (e) {
    console.warn("[syncWallpaperWindows] Failed to query existing windows:", e);
  }

  const monitorMap = new Map(
    monitors.map((m) => [m.name ?? `monitor_${monitors.indexOf(m)}`, m]),
  );

  const activeIds = new Set<string>();

  for (const config of configs) {
    if (!config.active || !config.wallpaper_id) continue;

    const monitor = monitorMap.get(config.monitor_id);
    if (!monitor) continue;

    activeIds.add(config.monitor_id);

    // 已创建则跳过
    if (activeWallpaperWindows.has(config.monitor_id)) continue;

    try {
      const pos = monitor.position;
      const size = monitor.size;

      await createWallpaperWindow(
        config.monitor_id,
        pos.x,
        pos.y,
        size.width,
        size.height,
      );
      activeWallpaperWindows.add(config.monitor_id);
      console.log(`[syncWallpaperWindows] Created window for ${config.monitor_id}`);
    } catch (e) {
      console.error(`[syncWallpaperWindows] Failed to create window for ${config.monitor_id}:`, e);
    }
  }

  // 销毁不再 active 的壁纸窗口
  for (const monitorId of activeWallpaperWindows) {
    if (!activeIds.has(monitorId)) {
      try {
        await destroyWallpaperWindow(monitorId);
        activeWallpaperWindows.delete(monitorId);
        console.log(`[syncWallpaperWindows] Destroyed window for ${monitorId}`);
      } catch (e) {
        console.error(`[syncWallpaperWindows] Failed to destroy window for ${monitorId}:`, e);
      }
    }
  }
}

/** 核心同步逻辑：检测物理显示器 → 比对 DB → upsert → 同步壁纸窗口 → 刷新 store */
export async function doSyncMonitors(
  set: (partial: { configs: MonitorConfig[] }) => void,
) {
  // 1. 检测当前物理显示器
  const monitors = await availableMonitors();
  const activeMonitorIds = new Set(
    monitors.map((m) => m.name ?? `monitor_${monitors.indexOf(m)}`),
  );

  // 2. 获取现有 configs
  const existingConfigs = await getMonitorConfigs();
  const existingIds = new Set(existingConfigs.map((c) => c.monitor_id));

  // 3. 批量 upsert：激活匹配的、关闭不匹配的、新增不存在的
  const upsertPromises: Promise<MonitorConfig>[] = [];

  for (const config of existingConfigs) {
    const shouldBeActive = activeMonitorIds.has(config.monitor_id);
    if (config.active !== shouldBeActive) {
      upsertPromises.push(
        upsertMonitorConfig({
          monitorId: config.monitor_id,
          active: shouldBeActive,
        }),
      );
    }
  }

  for (const mid of activeMonitorIds) {
    if (!existingIds.has(mid)) {
      upsertPromises.push(
        upsertMonitorConfig({
          monitorId: mid,
          active: true,
        }),
      );
    }
  }

  if (upsertPromises.length > 0) {
    await Promise.all(upsertPromises);
  }

  // 4. 重新获取最新 configs
  const configs = await getMonitorConfigs();
  set({ configs });

  // 5. 同步壁纸窗口（创建/销毁）
  await syncWallpaperWindows(configs, monitors);
}
