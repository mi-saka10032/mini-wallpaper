import { useCallback, useEffect } from "react";
import { useSettingStore, SETTING_KEYS } from "@/stores/settingStore";

/**
 * 预设主题色列表
 * 每项包含名称标识和色相值 (oklch hue)
 * "default" 表示无彩色（灰阶），hue 无意义
 */
export interface AccentPreset {
  id: string;
  hue: number;
  chroma: number; // oklch chroma 值
  label: string; // i18n key
}

export const ACCENT_PRESETS: AccentPreset[] = [
  { id: "default", hue: 0, chroma: 0, label: "accentColor.default" },
  { id: "blue", hue: 250, chroma: 0.15, label: "accentColor.blue" },
  { id: "purple", hue: 295, chroma: 0.15, label: "accentColor.purple" },
  { id: "pink", hue: 350, chroma: 0.15, label: "accentColor.pink" },
  { id: "red", hue: 25, chroma: 0.18, label: "accentColor.red" },
  { id: "orange", hue: 55, chroma: 0.16, label: "accentColor.orange" },
  { id: "green", hue: 145, chroma: 0.14, label: "accentColor.green" },
  { id: "teal", hue: 185, chroma: 0.12, label: "accentColor.teal" },
  { id: "cyan", hue: 210, chroma: 0.12, label: "accentColor.cyan" },
];

/** 存储格式: "presetId" 或 "custom:hue:chroma" */
export type AccentColorValue = string;

interface AccentColorConfig {
  hue: number;
  chroma: number;
  isDefault: boolean;
}

/**
 * 解析存储的 accent color 值
 */
function parseAccentValue(value: string | undefined): AccentColorConfig {
  if (!value || value === "default") {
    return { hue: 0, chroma: 0, isDefault: true };
  }

  // 自定义格式: "custom:hue:chroma"
  if (value.startsWith("custom:")) {
    const parts = value.split(":");
    return {
      hue: Number(parts[1]) || 0,
      chroma: Number(parts[2]) || 0.15,
      isDefault: false,
    };
  }

  // 预设 ID
  const preset = ACCENT_PRESETS.find((p) => p.id === value);
  if (preset) {
    return {
      hue: preset.hue,
      chroma: preset.chroma,
      isDefault: preset.id === "default",
    };
  }

  return { hue: 0, chroma: 0, isDefault: true };
}

/**
 * 根据色相和明暗模式生成 CSS 变量
 * 使用 oklch 色彩空间确保感知均匀
 */
function generateAccentVariables(
  config: AccentColorConfig,
  isDark: boolean,
): Record<string, string> {
  const { hue, chroma, isDefault } = config;

  if (isDefault) {
    // 返回空对象，让 CSS 中的默认值生效
    return {};
  }

  const h = hue;
  const c = chroma;

  if (isDark) {
    return {
      "--primary": `oklch(0.82 ${c} ${h})`,
      "--primary-foreground": `oklch(0.18 ${c * 0.3} ${h})`,
      "--primary-hover": `oklch(0.82 ${c} ${h} / 0.1)`,
      "--primary-hover-deep": `oklch(0.82 ${c} ${h} / 0.14)`,
      "--accent": `oklch(0.28 ${c * 0.5} ${h})`,
      "--accent-foreground": `oklch(0.9 ${c * 0.4} ${h})`,
      "--ring": `oklch(0.6 ${c * 0.8} ${h})`,
      "--sidebar-primary": `oklch(0.82 ${c} ${h})`,
      "--sidebar-primary-foreground": `oklch(0.18 ${c * 0.3} ${h})`,
      "--sidebar-accent": `oklch(0.28 ${c * 0.5} ${h})`,
      "--sidebar-accent-foreground": `oklch(0.9 ${c * 0.4} ${h})`,
      "--sidebar-ring": `oklch(0.6 ${c * 0.8} ${h})`,
    };
  } else {
    return {
      "--primary": `oklch(0.45 ${c} ${h})`,
      "--primary-foreground": `oklch(0.98 ${c * 0.1} ${h})`,
      "--primary-hover": `oklch(0.45 ${c} ${h} / 0.1)`,
      "--primary-hover-deep": `oklch(0.45 ${c} ${h} / 0.14)`,
      "--accent": `oklch(0.94 ${c * 0.4} ${h})`,
      "--accent-foreground": `oklch(0.3 ${c * 0.8} ${h})`,
      "--ring": `oklch(0.55 ${c * 0.8} ${h})`,
      "--sidebar-primary": `oklch(0.45 ${c} ${h})`,
      "--sidebar-primary-foreground": `oklch(0.98 ${c * 0.1} ${h})`,
      "--sidebar-accent": `oklch(0.94 ${c * 0.4} ${h})`,
      "--sidebar-accent-foreground": `oklch(0.3 ${c * 0.8} ${h})`,
      "--sidebar-ring": `oklch(0.55 ${c * 0.8} ${h})`,
    };
  }
}

/**
 * 将 CSS 变量应用到 document
 */
function applyAccentVariables(variables: Record<string, string>) {
  const root = document.documentElement;

  // 先清除之前设置的 accent 变量
  const accentKeys = [
    "--primary",
    "--primary-foreground",
    "--primary-hover",
    "--primary-hover-deep",
    "--accent",
    "--accent-foreground",
    "--ring",
    "--sidebar-primary",
    "--sidebar-primary-foreground",
    "--sidebar-accent",
    "--sidebar-accent-foreground",
    "--sidebar-ring",
  ];

  accentKeys.forEach((key) => {
    root.style.removeProperty(key);
  });

  // 应用新变量
  Object.entries(variables).forEach(([key, value]) => {
    root.style.setProperty(key, value);
  });
}

/**
 * 获取当前是否为暗色模式
 */
function isDarkMode(): boolean {
  return document.documentElement.classList.contains("dark");
}

/**
 * useAccentColor hook
 *
 * 优化：消除冗余的 useState，直接从 store 派生 accentValue，
 * 避免 store ↔ state 双源同步问题和多余的 re-render。
 */
export function useAccentColor() {
  // 直接从 store 精确订阅 accent_color 值，不再维护本地 state
  const accentValue = useSettingStore(
    (s) => s.settings[SETTING_KEYS.ACCENT_COLOR] || "default",
  );
  const updateSetting = useSettingStore((s) => s.updateSetting);

  // 应用 accent color（监听 accentValue 变化 + 明暗模式变化）
  useEffect(() => {
    const config = parseAccentValue(accentValue);
    const variables = generateAccentVariables(config, isDarkMode());
    applyAccentVariables(variables);

    // 监听 dark class 变化，重新应用
    const observer = new MutationObserver(() => {
      const newVariables = generateAccentVariables(config, isDarkMode());
      applyAccentVariables(newVariables);
    });

    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });

    return () => observer.disconnect();
  }, [accentValue]);

  const setAccentColor = useCallback(
    (value: string) => {
      updateSetting(SETTING_KEYS.ACCENT_COLOR, value);
    },
    [updateSetting],
  );

  /** 设置自定义颜色 */
  const setCustomColor = useCallback(
    (hue: number, chroma: number = 0.15) => {
      const value = `custom:${Math.round(hue)}:${chroma.toFixed(3)}`;
      setAccentColor(value);
    },
    [setAccentColor],
  );

  return {
    accentValue,
    setAccentColor,
    setCustomColor,
    presets: ACCENT_PRESETS,
    currentConfig: parseAccentValue(accentValue),
  };
}
