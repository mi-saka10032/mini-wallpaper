import { invoke } from "@tauri-apps/api/core";
import zhJson from "./toast-zh.json";
import enJson from "./toast-en.json";

type Dict = Record<string, string>;

const zh: Dict = zhJson;
const en: Dict = enJson;
const DICTS: Record<string, Dict> = { zh, en };

/** language 存储值可能是 "zh-CN" / "zh" / "en"，取主语言标签回退到 zh */
function resolveDict(lang: string | null): Dict {
  const tag = (lang ?? "").split("-")[0].toLowerCase();
  return DICTS[tag] ?? zh;
}

let dict: Dict = zh;

export async function initToastI18n(): Promise<void> {
  try {
    const lang = await invoke<string | null>("get_setting", { req: { key: "language" } });
    dict = resolveDict(lang);
  } catch {
    dict = zh;
  }
}

export function t(key: string): string {
  return dict[key] ?? zh[key] ?? key;
}
