import { invoke } from "@/api/invoke";
import { COMMANDS } from "@/api/config";

/** 获取所有设置（key-value 对象） */
export async function getSettings(): Promise<Record<string, string>> {
  return invoke(COMMANDS.GET_SETTINGS);
}

/** 获取单个设置值 */
export async function getSetting(key: string): Promise<string | null> {
  return invoke(COMMANDS.GET_SETTING, { key });
}

/** 设置键值对（可选 monitorId，用于 display_mode 变更时指定基准显示器） */
export async function setSetting(key: string, value: string, monitorId?: string): Promise<void> {
  return invoke(COMMANDS.SET_SETTING, { key, value, monitorId });
}
