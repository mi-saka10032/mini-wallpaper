import React from "react";
import { GripVertical } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";

interface SortToolbarProps {
  orderDirty: boolean;
  onCancel: () => void;
  onSave: () => void;
}

/** 排序模式下的操作栏 */
const SortToolbar: React.FC<SortToolbarProps> = React.memo(({
  orderDirty,
  onCancel,
  onSave,
}) => {
  const { t } = useTranslation();

  return (
    <>
      <GripVertical className="size-3.5 text-muted-foreground" />
      <span className="text-sm text-muted-foreground">{t("main.sortModeHint")}</span>
      {orderDirty && (
        <span className="ml-1 text-xs text-primary">{t("main.orderModified")}</span>
      )}
      <div className="flex-1" />
      <Button variant="ghost" size="sm" onClick={onCancel}>
        {t("main.cancel")}
      </Button>
      <Button variant="outline" size="sm" onClick={onSave} disabled={!orderDirty}>
        {t("main.save")}
      </Button>
    </>
  );
});

SortToolbar.displayName = "SortToolbar";

export default SortToolbar;
