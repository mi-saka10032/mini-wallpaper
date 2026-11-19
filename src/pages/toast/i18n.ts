import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { COMMANDS, type CommandMap } from "@/api/config";
import zh from "./toast-zh.json";
import en from "./toast-en.json";

i18n.use(initReactI18next).init({
  resources: {
    zh: { translation: zh },
    en: { translation: en },
  },
  lng: "zh",
  fallbackLng: "zh",
  interpolation: {
    escapeValue: false,
  },
});

export async function initToastI18n() {
  const lang = await invoke<CommandMap[typeof COMMANDS.GET_SETTING]["result"]>(
    COMMANDS.GET_SETTING,
    { req: { key: "language" } },
  ).catch(() => null);
  if (lang) await i18n.changeLanguage(lang);
}

export default i18n;
