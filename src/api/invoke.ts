import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import type { CommandMap } from "./config";
import { toast } from "@/components/ui/toast";

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
 * 所有调用自动 try...catch，错误时通过顶部 Toast 弹出提示。
 * 如需静默处理错误（自行 catch），可传入 `{ silent: true }` 作为最后一个参数。
 */

/** invoke 选项 */
interface InvokeOptions {
  /** 为 true 时不弹出错误 Toast，由调用方自行处理 */
  silent?: boolean;
}

export async function invoke<K extends keyof CommandMap>(
  command: K,
  ...args: HasParams<K> extends true
    ? [ExtractReq<CommandMap[K]["params"]>] | [ExtractReq<CommandMap[K]["params"]>, InvokeOptions]
    : [] | [InvokeOptions]
): Promise<CommandMap[K]["result"]> {
  // 解析参数：最后一个参数如果是 InvokeOptions 则提取出来
  let reqArg: unknown = undefined;
  let options: InvokeOptions = {};

  if (args.length > 0) {
    const last = args[args.length - 1];
    if (last != null && typeof last === "object" && "silent" in last) {
      options = last as InvokeOptions;
      reqArg = args.length > 1 ? args[0] : undefined;
    } else {
      reqArg = args[0];
    }
  }

  try {
    if (reqArg === undefined) {
      return await tauriInvoke(command);
    }
    return await tauriInvoke(command, { req: reqArg });
  } catch (err: unknown) {
    const message = typeof err === "string" ? err : (err as Error)?.message ?? String(err);
    if (!options.silent) {
      toast.error(`[${command}] ${message}`);
    }
    throw err;
  }
}