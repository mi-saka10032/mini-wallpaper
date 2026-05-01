import { useCallback, useEffect, useRef } from "react";
import {
  register,
  unregister,
  unregisterAll,
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
 *
 * 优化：仅精确订阅快捷键相关的两个 setting key，避免其他设置
 * （如音量、主题等）变化时触发不必要的快捷键重注册。
 */
export function useShortcuts() {
  // 精确订阅：仅监听快捷键相关的 setting 值
  const shortcutNext = useSettingStore(
    (s) => s.settings[SETTING_KEYS.SHORTCUT_NEXT] || DEFAULT_SHORTCUTS.nextWallpaper,
  );
  const shortcutPrev = useSettingStore(
    (s) => s.settings[SETTING_KEYS.SHORTCUT_PREV] || DEFAULT_SHORTCUTS.prevWallpaper,
  );
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
    // 先注销所有已注册的快捷键（包括 webview 刷新后后端残留的注册）
    try {
      await unregisterAll();
    } catch {
      // 如果 unregisterAll 失败，尝试逐个注销
      for (const shortcut of registeredRef.current) {
        try { await unregister(shortcut); } catch { /* ignore */ }
      }
    }
    registeredRef.current = [];

    // 构建当前绑定映射
    const bindings: { binding: string; action: keyof typeof DEFAULT_SHORTCUTS; direction: "next" | "prev" }[] = [
      { binding: shortcutNext, action: "nextWallpaper", direction: "next" },
      { binding: shortcutPrev, action: "prevWallpaper", direction: "prev" },
    ];

    for (const { binding, action, direction } of bindings) {
      if (!binding) continue;
      // 跳过重复
      if (registeredRef.current.includes(binding)) continue;

      const handler = throttled(direction, () =>
        invoke(COMMANDS.SWITCH_WALLPAPER, { direction }),
      );

      // 注册快捷键，带重试机制
      let registered = false;
      for (let attempt = 0; attempt < 2; attempt++) {
        try {
          await register(binding, handler);
          registeredRef.current.push(binding);
          registered = true;
          break;
        } catch {
          // 首次失败后等待一小段时间再重试（给系统释放快捷键的时间）
          if (attempt === 0) {
            await new Promise((r) => setTimeout(r, 100));
            // 再次尝试注销
            try { await unregister(binding); } catch { /* ignore */ }
          }
        }
      }

      // 重试仍失败：回退到默认值
      if (!registered) {
        const fallback = DEFAULT_SHORTCUTS[action];
        if (binding !== fallback) {
          console.warn(`[useShortcuts] "${binding}" unavailable, falling back to "${fallback}"`);
          try {
            try { await unregister(fallback); } catch { /* ignore */ }
            await register(fallback, handler);
            registeredRef.current.push(fallback);
            // 把坏值修正回默认
            const settingKey = ACTIONS.find((a) => a.action === action)?.settingKey;
            if (settingKey) updateSetting(settingKey, fallback);
          } catch (e2) {
            console.warn(`[useShortcuts] Fallback "${fallback}" also failed:`, e2);
          }
        }
      }
    }
  }, [shortcutNext, shortcutPrev, throttled, updateSetting]);

  // 快捷键绑定变化时重新注册
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