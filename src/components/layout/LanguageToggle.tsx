import { Languages } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { changeLanguage } from "@/i18n";
import { useSettingStore, SETTING_KEYS } from "@/stores/settingStore";

const LanguageToggle: React.FC = () => {
  const { t, i18n } = useTranslation();
  const updateSetting = useSettingStore((s) => s.updateSetting);

  const handleChange = (lang: string) => {
    changeLanguage(lang);
    updateSetting(SETTING_KEYS.LANGUAGE, lang);
  };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon-sm">
          <Languages className="size-4" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuItem
          onClick={() => handleChange("zh")}
          className={i18n.language === "zh" ? "bg-primary-hover-deep font-medium" : ""}
        >
          <span>{t("language.zh")}</span>
        </DropdownMenuItem>
        <DropdownMenuItem
          onClick={() => handleChange("en")}
          className={i18n.language === "en" ? "bg-primary-hover-deep font-medium" : ""}
        >
          <span>{t("language.en")}</span>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
};

export default LanguageToggle;
