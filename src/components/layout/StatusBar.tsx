import { memo, type FC } from "react";
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
const StatusBar: FC<StatusBarProps> = memo(({
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
    <div className="flex h-7 shrink-0 items-center border-t border-border/40 px-4">
      <span className="text-[11px] text-foreground/45">
        {manageMode && selectedCount > 0
          ? t("main.selectedTotal", { selected: selectedCount, total: displayCount })
          : t("main.total", { count: displayCount })}
      </span>
      {manageMode && keyword && displayCount !== totalCount && (
        <span className="ml-2 text-[11px] text-foreground/45">
          {t("grid.filterResult", { filtered: displayCount, total: totalCount })}
        </span>
      )}
      {!manageMode && !sortMode && normalKeyword && displayCount !== totalCount && (
        <span className="ml-2 text-[11px] text-foreground/45">
          {t("grid.filterResult", { filtered: displayCount, total: totalCount })}
        </span>
      )}
      {manageMode && pendingRemovalsCount > 0 && (
        <span className="ml-2 text-[11px] text-orange-500/80">
          {t("main.pendingRemovals", { count: pendingRemovalsCount })}
        </span>
      )}
      {manageMode && pendingDeletionsCount > 0 && (
        <span className="ml-2 text-[11px] text-orange-500/80">
          {t("main.pendingDeletions", { count: pendingDeletionsCount })}
        </span>
      )}
    </div>
  );
});

StatusBar.displayName = "StatusBar";

export default StatusBar;