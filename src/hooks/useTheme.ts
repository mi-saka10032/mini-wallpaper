import { useCallback, useEffect, useState } from "react";
import { useSettingStore, SETTING_KEYS } from "@/stores/settingStore";

type Theme = "light" | "dark" | "system";

function getSystemTheme(): "light" | "dark" {
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function applyTheme(theme: Theme) {
  const resolved = theme === "system" ? getSystemTheme() : theme;
  document.documentElement.classList.toggle("dark", resolved === "dark");
}

export function useTheme() {
  const storedTheme = useSettingStore(
    (s) => s.settings[SETTING_KEYS.THEME] as Theme | undefined,
  );
  const updateSetting = useSettingStore((s) => s.updateSetting);

  const [theme, setThemeState] = useState<Theme>(storedTheme || "system");

  // 当 store 中的 theme 变化时同步
  useEffect(() => {
    if (storedTheme) {
      setThemeState(storedTheme);
      applyTheme(storedTheme);
    }
  }, [
    
  ]);

  const setTheme = useCallback(
    (newTheme: Theme) => {
      setThemeState(newTheme);
      applyTheme(newTheme);
      updateSetting(SETTING_KEYS.THEME, newTheme);
    },
    [updateSetting],
  );

  // 初始化 + 监听系统主题变化
  useEffect(() => {
    applyTheme(theme);

    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    const handleChange = () => {
      if (theme === "system") {
        applyTheme("system");
      }
    };
    mediaQuery.addEventListener("change", handleChange);
    return () => mediaQuery.removeEventListener("change", handleChange);
  }, [theme]);

  return { theme, setTheme };
}
