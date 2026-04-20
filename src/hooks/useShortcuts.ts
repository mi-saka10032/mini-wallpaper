import { useCallback, useEffect, useRef } from "react";
import {
  register,
  unregister,
  isRegistered,
} from "@tauri-apps/plugin-global-shortcut";
import { invoke } from "@/api/invoke";
import { COMMANDS } from "@/api/config";
import { useSettingStore, SETTING_KEYS } from "@/stores/settingStore";

/** 默认快捷键 */
export const DEFAULT_SHORTCUTS = {
  nextWallpaper: "CommandOrControl+Alt+N",
  prevWallpaper: "CommandOrControl+Alt+P",
} as const;

/** action → setting key 映射 */
const ACTIONS: { action: keyof typeof DEFAULT_SHORTCUTS; settingKey: string; direction: "next" | "prev" }[] = [
  { action: "nextWallpaper", settingKey: SETTING_KEYS.SHORTCUT_NEXT, direction: "next" },
  { action: "prevWallpaper", settingKey: SETTING_KEYS.SHORTCUT_PREV, direction: "prev" },
];

/** 节流间隔（ms） */
const THROTTLE_MS = 500;

/**
 * 全局快捷键管理 hook
 *
 * 在 App 根组件调用一次。读取 app_settings 中保存的快捷键绑定，
 * 注册到系统全局快捷键。窗口隐藏/托盘常驻时仍然生效。
 */
export function useShortcuts() {
  const settings = useSettingStore((s) => s.settings);
  const updateSetting = useSettingStore((s) => s.updateSetting);
  const registeredRef = useRef<string[]>([]);
  const lastFireRef = useRef<Record<string, number>>({});

  /** 带节流的 handler 工厂 */
  const throttled = useCallback(
    (key: string, fn: () => void) => {
      return () => {
        const now = Date.now();
        if (now - (lastFireRef.current[key] ?? 0) < THROTTLE_MS) return;
        lastFireRef.current[key] = now;
        fn();
      };
    },
    [],
  );

  // 注册所有快捷键
  const registerAll = useCallback(async () => {
    // 先注销旧的
    for (const shortcut of registeredRef.current) {
      try {
        if (await isRegistered(shortcut)) {
          await unregister(shortcut);
        }
      } catch {
        // ignore
      }
    }
    registeredRef.current = [];

    for (const { action, settingKey, direction } of ACTIONS) {
      const binding = settings[settingKey] || DEFAULT_SHORTCUTS[action];
      if (!binding) continue;
      // 跳过重复
      if (registeredRef.current.includes(binding)) continue;

      const handler = throttled(direction, () =>
        invoke(COMMANDS.SWITCH_WALLPAPER, { direction }),
      );

      try {
        if (await isRegistered(binding)) {
          await unregister(binding);
        }
        await register(binding, handler);
        registeredRef.current.push(binding);
      } catch {
        // 注册失败：DB 里的值是坏的，回退到默认值重试
        const fallback = DEFAULT_SHORTCUTS[action];
        if (binding !== fallback) {
          console.warn(`[useShortcuts] "${binding}" unavailable, falling back to "${fallback}"`);
          try {
            if (await isRegistered(fallback)) {
              await unregister(fallback);
            }
            await register(fallback, handler);
            registeredRef.current.push(fallback);
            // 把坏值修正回默认
            updateSetting(settingKey, fallback);
          } catch (e2) {
            console.warn(`[useShortcuts] Fallback "${fallback}" also failed:`, e2);
          }
        }
      }
    }
  }, [settings, throttled, updateSetting]);

  // 设置变化时重新注册
  useEffect(() => {
    registerAll();
    return () => {
      for (const shortcut of registeredRef.current) {
        unregister(shortcut).catch(() => {});
      }
      registeredRef.current = [];
    };
  }, [registerAll]);
}
