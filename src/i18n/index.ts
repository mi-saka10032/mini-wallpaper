import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import zh from "./zh";
import en from "./en";

i18n.use(initReactI18next).init({
  resources: {
    zh: { translation: zh },
    en: { translation: en },
  },
  lng: "zh", // 默认中文，启动后由 settingStore 覆盖
  fallbackLng: "zh",
  interpolation: {
    escapeValue: false,
  },
});

/** 切换语言（由 settingStore.updateSetting 负责持久化） */
export function changeLanguage(lang: string) {
  i18n.changeLanguage(lang);
}

export default i18n;
