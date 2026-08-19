import { Trash2 } from "lucide-react";
import { memo, useCallback, useMemo, useState, type FC } from "react";
import { useTranslation } from "react-i18next";

import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { SETTING_KEYS, useSetting, useUpdateSetting } from "@/stores/settingStore";

/** 默认保留天数，与后端 DEFAULT_TRASH_RETENTION_DAYS 保持一致 */
const DEFAULT_RETENTION_DAYS = 30;

/** 后端校验的取值范围（app_setting_dto.rs） */
const MIN_DAYS = 1;
const MAX_DAYS = 3650;

/**
 * 回收站设置区块
 *
 * 支持自定义保留天数与关闭自动清理。关闭后回收站内容永久保留，
 * 仅能通过回收站视图手动清空。
 */
const TrashSection: FC = memo(() => {
  const { t } = useTranslation();
  const updateSetting = useUpdateSetting();

  const retentionRaw = useSetting(SETTING_KEYS.TRASH_RETENTION_DAYS);
  const autoPurgeRaw = useSetting(SETTING_KEYS.TRASH_AUTO_PURGE);

  // 缺省视为启用，与后端启动清理的缺省行为一致
  const autoPurgeEnabled = autoPurgeRaw !== "false";

  const storedDays = useMemo(() => {
    const parsed = retentionRaw ? Number.parseInt(retentionRaw, 10) : Number.NaN;
    return Number.isFinite(parsed) && parsed > 0 ? parsed : DEFAULT_RETENTION_DAYS;
  }, [retentionRaw]);

  // 输入框本地态：允许用户输入中间态（如清空重打），失焦时才校验落库
  const [draft, setDraft] = useState<string | null>(null);
  const [error, setError] = useState("");

  const inputValue = draft ?? String(storedDays);

  const handleToggleAutoPurge = useCallback(
    (checked: boolean) => {
      updateSetting(SETTING_KEYS.TRASH_AUTO_PURGE, checked ? "true" : "false");
    },
    [updateSetting],
  );

  const commitDays = useCallback(() => {
    // 未编辑过：无需落库
    if (draft === null) return;

    const parsed = Number.parseInt(draft, 10);
    if (!Number.isFinite(parsed) || parsed < MIN_DAYS || parsed > MAX_DAYS) {
      setError(t("trash.retentionRange"));
      // 回退到已保存值，避免留下非法中间态
      setDraft(null);
      return;
    }

    setError("");
    setDraft(null);
    if (parsed !== storedDays) {
      updateSetting(SETTING_KEYS.TRASH_RETENTION_DAYS, String(parsed));
    }
  }, [draft, storedDays, updateSetting, t]);

  return (
    <section className="space-y-5">
      <div>
        <h3 className="text-[15px] font-semibold text-foreground">{t("trash.settingsTitle")}</h3>
        <p className="mt-1 text-[11px] leading-relaxed text-foreground/45">
          {t("trash.settingsDesc")}
        </p>
      </div>

      <div className="rounded-lg border border-border/50 bg-card">
        {/* 自动清理开关 */}
        <div className="flex items-center justify-between px-4 py-3.5">
          <div className="flex items-center gap-2.5">
            <Trash2 className="size-4 text-foreground/50" />
            <span className="text-[13px] text-foreground/80">{t("trash.autoPurgeLabel")}</span>
          </div>
          <Switch checked={autoPurgeEnabled} onCheckedChange={handleToggleAutoPurge} />
        </div>

        <div className="mx-4 h-px bg-border/30" />

        {/* 保留天数 */}
        <div className="flex items-center justify-between gap-3 px-4 py-3.5">
          <span
            className={
              autoPurgeEnabled
                ? "text-[13px] text-foreground/80"
                : "text-[13px] text-foreground/35"
            }
          >
            {t("trash.retentionLabel")}
          </span>
          <div className="flex items-center gap-2">
            <Input
              type="number"
              min={MIN_DAYS}
              max={MAX_DAYS}
              value={inputValue}
              disabled={!autoPurgeEnabled}
              onChange={(e) => {
                setDraft(e.target.value);
                if (error) setError("");
              }}
              onBlur={commitDays}
              onKeyDown={(e) => {
                if (e.key === "Enter") commitDays();
              }}
              className="h-8 w-20 text-right text-[13px] tabular-nums"
            />
            <span
              className={
                autoPurgeEnabled
                  ? "text-[12px] text-foreground/60"
                  : "text-[12px] text-foreground/30"
              }
            >
              {t("trash.retentionUnit")}
            </span>
          </div>
        </div>
      </div>

      {error && <p className="px-1 text-[11px] text-destructive">{error}</p>}

      <p className="px-1 text-[11px] leading-relaxed text-foreground/45">
        {autoPurgeEnabled
          ? t("trash.retentionHint", { days: storedDays })
          : t("trash.retentionDisabledHint")}
      </p>
    </section>
  );
});

TrashSection.displayName = "TrashSection";

export default TrashSection;
