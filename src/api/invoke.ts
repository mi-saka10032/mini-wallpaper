import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import type { CommandMap } from "./config";

/**
 * 类型安全的 invoke 封装
 * command 名、入参、出参由 CommandMap 自动推导
 */
export async function invoke<K extends keyof CommandMap>(
  command: K,
  ...args: CommandMap[K]["params"] extends Record<string, never> ? [] : [CommandMap[K]["params"]]
): Promise<CommandMap[K]["result"]> {
  return tauriInvoke(command, args[0]);
}
