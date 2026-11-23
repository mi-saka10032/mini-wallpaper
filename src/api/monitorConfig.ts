import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { COMMANDS } from "@/api/config";
import type { MonitorConfig, UpsertMonitorConfigReq } from "@/api/config";
import type { ExtendViewportPayload } from "@/api/event";
import { invoke } from "@/api/invoke";

/** 获取所有显示器配置 */
export async function getMonitorConfigs(): Promise<MonitorConfig[]> {
  return invoke(COMMANDS.GET_MONITOR_CONFIGS);
}

/** 根据 monitor_id 获取配置 */
export async function getMonitorConfig(monitorId: string): Promise<MonitorConfig | null> {
  return invoke(COMMANDS.GET_MONITOR_CONFIG, { monitorId });
}

/** 拉取当前显示器 extend 视口（壁纸页挂载时用，避免错过事件） */
export async function getExtendViewport(monitorId: string): Promise<ExtendViewportPayload | null> {
  // 读命令，直连 tauri invoke：本项目的 invoke 封装约定 req 包裹，
  // 而此命令只收裸 monitor_id，无需 toast / 失效信号。
  return tauriInvoke<ExtendViewportPayload | null>(COMMANDS.GET_EXTEND_VIEWPORT, { monitorId });
}

/** 创建或更新显示器配置 */
export async function upsertMonitorConfig(params: UpsertMonitorConfigReq): Promise<MonitorConfig> {
  return invoke(COMMANDS.UPSERT_MONITOR_CONFIG, params);
}

/** 删除显示器配置 */
export async function deleteMonitorConfig(id: number, monitorId?: string): Promise<void> {
  return invoke(COMMANDS.DELETE_MONITOR_CONFIG, { id, monitorId });
}
