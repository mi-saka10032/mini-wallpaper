import React from "react";
import { useTranslation } from "react-i18next";

interface StatusBarProps {
  manageMode: boolean;
  sortMode: boolean;
  selectedCount: number;
  displayCount: number;
  totalCount: number;
  keyword: string;
  normalKeyword: string;
  pendingRemovalsCount: number;
  pendingDeletionsCount: number;
}

/** 底部状态栏 */
const StatusBar: React.FC<StatusBarProps> = React.memo(({
  manageMode,
  sortMode,
  selectedCount,
  displayCount,
  totalCount,
  keyword,
  normalKeyword,
  pendingRemovalsCount,
  pendingDeletionsCount,
}) => {
  const { t } = useTranslation();

  return (
    <div className="flex h-8 shrink-0 items-center border-t border-border px-4">
      <span className="text-xs text-muted-foreground">
        {manageMode && selectedCount > 0
          ? t("main.selectedTotal", { selected: selectedCount, total: displayCount })
          : t("main.total", { count: displayCount })}
      </span>
      {manageMode && keyword && displayCount !== totalCount && (
        <span className="ml-2 text-xs text-muted-foreground">
          {t("grid.filterResult", { filtered: displayCount, total: totalCount })}
        </span>
      )}
      {!manageMode && !sortMode && normalKeyword && displayCount !== totalCount && (
        <span className="ml-2 text-xs text-muted-foreground">
          {t("grid.filterResult", { filtered: displayCount, total: totalCount })}
        </span>
      )}
      {manageMode && pendingRemovalsCount > 0 && (
        <span className="ml-2 text-xs text-orange-500">
          {t("main.pendingRemovals", { count: pendingRemovalsCount })}
        </span>
      )}
      {manageMode && pendingDeletionsCount > 0 && (
        <span className="ml-2 text-xs text-orange-500">
          {t("main.pendingDeletions", { count: pendingDeletionsCount })}
        </span>
      )}
    </div>
  );
});

StatusBar.displayName = "StatusBar";

export default StatusBar;