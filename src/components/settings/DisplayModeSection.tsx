import React from "react";
import { useTranslation } from "react-i18next";
import { Layers, Link } from "lucide-react";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

interface DisplayModeSectionProps {
  displayMode: string;
  isSyncMode: boolean;
  onDisplayModeChange: (mode: string) => void;
}

/** 显示模式选择区块 */
const DisplayModeSection: React.FC<DisplayModeSectionProps> = React.memo(({
  displayMode,
  isSyncMode,
  onDisplayModeChange,
}) => {
  const { t } = useTranslation();

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <Layers className="size-4 text-muted-foreground" />
        <Label className="text-sm font-medium">{t("monitor.displayMode")}</Label>
      </div>
      <Select
        value={displayMode}
        onValueChange={onDisplayModeChange}
      >
        <SelectTrigger className="w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="independent">
            <div className="flex items-center gap-2">
              <span>{t("monitor.displayIndependent")}</span>
            </div>
          </SelectItem>
          <SelectItem value="mirror">
            <div className="flex items-center gap-2">
              <span>{t("monitor.displayMirror")}</span>
            </div>
          </SelectItem>
          <SelectItem value="extend">
            <div className="flex items-center gap-2">
              <span>{t("monitor.displayExtend")}</span>
            </div>
          </SelectItem>
        </SelectContent>
      </Select>
      <p className="text-xs text-muted-foreground">
        {displayMode === "mirror"
          ? t("monitor.displayMirrorDesc")
          : displayMode === "extend"
            ? t("monitor.displayExtendDesc")
            : t("monitor.displayIndependentDesc")}
      </p>

      {/* 同步模式提示条 */}
      {isSyncMode && (
        <div className="flex items-center gap-2 rounded-lg border border-primary/30 bg-primary/5 px-3 py-2">
          <Link className="size-3.5 shrink-0 text-primary" />
          <span className="text-xs text-primary">
            {t("monitor.displaySyncHint", {
              mode: displayMode === "mirror"
                ? t("monitor.displayMirror")
                : t("monitor.displayExtend"),
            })}
          </span>
        </div>
      )}
    </div>
  );
});

DisplayModeSection.displayName = "DisplayModeSection";

export default DisplayModeSection;
