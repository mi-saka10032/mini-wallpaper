import { COMMANDS } from "@/api/config";
import type { MonitorConfig, UpsertMonitorConfigReq } from "@/api/config";
import { invoke } from "@/api/invoke";

/** 获取所有显示器配置 */
export async function getMonitorConfigs(): Promise<MonitorConfig[]> {
  return invoke(COMMANDS.GET_MONITOR_CONFIGS);
}

/** 根据 monitor_id 获取配置 */
export async function getMonitorConfig(monitorId: string): Promise<MonitorConfig | null> {
  return invoke(COMMANDS.GET_MONITOR_CONFIG, { monitorId });
}

/** 创建或更新显示器配置 */
export async function upsertMonitorConfig(params: UpsertMonitorConfigReq): Promise<MonitorConfig> {
  return invoke(COMMANDS.UPSERT_MONITOR_CONFIG, params);
}

/** 删除显示器配置 */
export async function deleteMonitorConfig(id: number, monitorId?: string): Promise<void> {
  return invoke(COMMANDS.DELETE_MONITOR_CONFIG, { id, monitorId });
}
