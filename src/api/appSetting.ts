import { invoke as tauriInvoke } from "@tauri-apps/api/core";
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
  // 直接调用 tauriInvoke，因为 set_setting 命令有两个独立参数：req 和 monitor_id
  return tauriInvoke(COMMANDS.SET_SETTING, {
    req: { key, value },
    monitorId: monitorId ?? null,
  });
}
