import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import type { CommandMap } from "./config";

/**
 * 从 CommandMap params 中提取 req 的类型
 * - 有 req 字段 → 提取 req 的类型
 * - Record<string, never>（无参数） → never
 */
type ExtractReq<P> = P extends { req: infer R } ? R : never;

/**
 * 判断 command 是否有参数
 */
type HasParams<K extends keyof CommandMap> =
  CommandMap[K]["params"] extends Record<string, never> ? false : true;

/**
 * 类型安全的 invoke 封装
 *
 * - 无参数的 command：直接调用 `invoke(COMMANDS.XXX)`
 * - 有参数的 command：调用 `invoke(COMMANDS.XXX, reqObj)`，内部自动包裹为 `{ req: reqObj }`
 *
 * 外部调用者只需关注请求对象本身，无需手动声明 `{ req: ... }` 包裹层。
 */
export async function invoke<K extends keyof CommandMap>(
  command: K,
  ...args: HasParams<K> extends true ? [ExtractReq<CommandMap[K]["params"]>] : []
): Promise<CommandMap[K]["result"]> {
  if (args.length === 0) {
    return tauriInvoke(command);
  }
  return tauriInvoke(command, { req: args[0] });
}
