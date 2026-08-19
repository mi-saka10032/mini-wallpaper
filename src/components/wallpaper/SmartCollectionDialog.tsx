import { Loader2, Plus } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { previewSmartCount } from "@/api/collection";
import { getTags } from "@/api/tag";
import type { Collection, RuleCombinator, RuleItem, SmartRule, TagWithCount } from "@/api/config";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { toast } from "@/components/ui/toast";
import { SmartRuleRow } from "@/components/wallpaper/SmartRuleRow";
import { TagInput } from "@/components/wallpaper/TagInput";
import { useCollectionStore } from "@/stores/collectionStore";
import {
  SIMPLE_PRESETS,
  TYPE_ENUM,
  newDefaultRule,
  smartRuleToText,
  validateSmartRule,
} from "@/lib/smartRuleMeta";
import { cn } from "@/lib/utils";

// ============ 类型定义 ============

export interface SmartCollectionDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** 传入则为「编辑」，否则为「创建」 */
  editing?: Collection | null;
}

type Tab = "simple" | "advanced";

/** 解析已有收藏夹的 rule_json（编辑态回填），失败返回空规则 */
function parseRule(json: string | null | undefined): SmartRule {
  if (!json) return { version: 1, combinator: "and", rules: [] };
  try {
    const parsed = JSON.parse(json) as SmartRule;
    return {
      version: parsed.version ?? 1,
      combinator: parsed.combinator === "or" ? "or" : "and",
      rules: Array.isArray(parsed.rules) ? parsed.rules : [],
    };
  } catch {
    return { version: 1, combinator: "and", rules: [] };
  }
}

/**
 * 智能收藏夹规则构建器对话框（创建 / 编辑共用）
 *
 * - 简单档：名称 + 标签 chips + 类型多选 + 预设开关（横屏/4K/最近7天）
 * - 高级档：AND/OR + 多条件三段式构建器 + 自然语言只读回显
 * - 两档共用同一份 rules 与同一校验；命中数实时预览（防抖）
 */
export const SmartCollectionDialog: React.FC<SmartCollectionDialogProps> = ({
  open,
  onOpenChange,
  editing = null,
}) => {
  const { t } = useTranslation();
  const createSmart = useCollectionStore((s) => s.createSmartCollection);
  const updateSmart = useCollectionStore((s) => s.updateSmartCollection);

  const [tab, setTab] = useState<Tab>("simple");
  const [name, setName] = useState("");
  const [combinator, setCombinator] = useState<RuleCombinator>("and");
  const [rules, setRules] = useState<RuleItem[]>([]);
  const [tags, setTags] = useState<TagWithCount[]>([]);
  const [saving, setSaving] = useState(false);

  // 命中数预览
  const [matchCount, setMatchCount] = useState<number | null>(null);
  const [previewing, setPreviewing] = useState(false);

  const tagNameById = useMemo(() => new Map(tags.map((tenta) => [tenta.id, tenta.name])), [tags]);

  // 组装当前规则对象
  const currentRule: SmartRule = useMemo(
    () => ({ version: 1, combinator, rules }),
    [combinator, rules],
  );

  const validationErrKey = useMemo(() => validateSmartRule(currentRule), [currentRule]);

  // 打开时初始化（编辑态回填 / 创建态清空），并加载标签
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    (async () => {
      try {
        const allTags = await getTags();
        if (cancelled) return;
        setTags(allTags);
      } catch (e) {
        console.error("[SmartCollectionDialog.loadTags]", e);
      }
    })();

    if (editing) {
      const parsed = parseRule(editing.rule_json);
      setName(editing.name);
      setCombinator(parsed.combinator);
      setRules(parsed.rules);
      // 有非简单可表达的组合子（or）或复杂规则时默认切到高级档
      setTab(parsed.combinator === "or" ? "advanced" : "simple");
    } else {
      setName("");
      setCombinator("and");
      setRules([]);
      setTab("simple");
    }
    setMatchCount(null);
    return () => {
      cancelled = true;
    };
  }, [open, editing]);

  // 命中数预览（防抖 400ms）
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    if (!open) return;
    if (validationErrKey) {
      setMatchCount(null);
      return;
    }
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(async () => {
      setPreviewing(true);
      try {
        const count = await previewSmartCount(JSON.stringify(currentRule));
        setMatchCount(count);
      } catch {
        setMatchCount(null);
      } finally {
        setPreviewing(false);
      }
    }, 400);
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [open, currentRule, validationErrKey]);

  // ===== 简单档操作 =====

  // 标签条件（简单档用 includes_any 单条）
  const simpleTagRuleIndex = rules.findIndex((r) => r.field === "tag");
  const simpleTagNames = useMemo(() => {
    if (simpleTagRuleIndex < 0) return [];
    const ids = (rules[simpleTagRuleIndex].value as number[]) ?? [];
    return ids.map((id) => tagNameById.get(id)).filter((n): n is string => n != null);
  }, [rules, simpleTagRuleIndex, tagNameById]);

  const setSimpleTags = (names: string[]) => {
    const idByName = new Map(tags.map((tenta) => [tenta.name, tenta.id]));
    const ids = names.map((n) => idByName.get(n)).filter((id): id is number => id != null);
    setRules((prev) => {
      const next = prev.filter((r) => r.field !== "tag");
      if (ids.length > 0) {
        next.push({ field: "tag", op: "includes_any", value: ids });
      }
      return next;
    });
  };

  // 类型条件（简单档：单选 type eq；空 = 不限）
  const simpleType = rules.find((r) => r.field === "type" && r.op === "eq")?.value as
    | string
    | undefined;
  const setSimpleType = (type: string | null) => {
    setRules((prev) => {
      const next = prev.filter((r) => r.field !== "type");
      if (type) next.push({ field: "type", op: "eq", value: type });
      return next;
    });
  };

  // 预设开关
  const togglePreset = (presetKey: string) => {
    const preset = SIMPLE_PRESETS.find((p) => p.key === presetKey);
    if (!preset) return;
    setRules((prev) => {
      const exists = prev.some((r) => preset.matches(r));
      if (exists) return prev.filter((r) => !preset.matches(r));
      return [...prev, preset.build()];
    });
  };

  // ===== 高级档操作 =====

  const addRule = () => setRules((prev) => [...prev, newDefaultRule()]);
  const updateRule = (idx: number, next: RuleItem) =>
    setRules((prev) => prev.map((r, i) => (i === idx ? next : r)));
  const removeRule = (idx: number) => setRules((prev) => prev.filter((_, i) => i !== idx));

  // ===== 保存 =====

  const handleSave = useCallback(async () => {
    const trimmedName = name.trim();
    if (!trimmedName) {
      toast.error(t("smart.errNameEmpty"));
      return;
    }
    if (validationErrKey) {
      toast.error(t(validationErrKey));
      return;
    }
    setSaving(true);
    try {
      const ruleJson = JSON.stringify(currentRule);
      if (editing) {
        await updateSmart(editing.id, ruleJson, trimmedName);
        toast.success(t("smart.updated"));
      } else {
        await createSmart(trimmedName, ruleJson);
        toast.success(t("smart.created"));
      }
      onOpenChange(false);
    } catch (e) {
      console.error("[SmartCollectionDialog.save]", e);
      // 具体错误已由 invoke 层 toast
    } finally {
      setSaving(false);
    }
  }, [name, validationErrKey, currentRule, editing, updateSmart, createSmart, onOpenChange, t]);

  const nlText = useMemo(
    () => smartRuleToText(currentRule, { t, tagNameById }),
    [currentRule, t, tagNameById],
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{editing ? t("smart.editTitle") : t("smart.createTitle")}</DialogTitle>
          <DialogDescription>{t("smart.desc")}</DialogDescription>
        </DialogHeader>

        {/* 名称 */}
        <div className="flex flex-col gap-1.5">
          <label className="text-xs font-medium text-foreground/70">{t("smart.nameLabel")}</label>
          <Input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder={t("smart.namePlaceholder")}
            maxLength={32}
            autoFocus
          />
        </div>

        {/* 档位切换 */}
        <div className="flex items-center gap-1 rounded-md bg-muted/50 p-0.5 text-sm">
          {(["simple", "advanced"] as Tab[]).map((tb) => (
            <button
              key={tb}
              type="button"
              onClick={() => setTab(tb)}
              className={cn(
                "flex-1 rounded px-3 py-1 transition-colors",
                tab === tb
                  ? "bg-background font-medium text-foreground shadow-sm"
                  : "text-foreground/55 hover:text-foreground",
              )}
            >
              {tb === "simple" ? t("smart.tabSimple") : t("smart.tabAdvanced")}
            </button>
          ))}
        </div>

        {/* 内容区 */}
        <div className="max-h-[42vh] overflow-y-auto pr-1">
          {tab === "simple" ? (
            <div className="flex flex-col gap-4">
              {/* 标签 */}
              <div className="flex flex-col gap-1.5">
                <label className="text-xs font-medium text-foreground/70">{t("smart.tagLabel")}</label>
                <TagInput
                  value={simpleTagNames}
                  onChange={setSimpleTags}
                  suggestions={tags.map((tenta) => tenta.name)}
                  placeholder={t("smart.tagPlaceholder")}
                />
                <p className="text-xs text-foreground/40">{t("smart.tagHint")}</p>
              </div>

              {/* 类型 */}
              <div className="flex flex-col gap-1.5">
                <label className="text-xs font-medium text-foreground/70">{t("smart.typeLabel")}</label>
                <div className="flex gap-1.5">
                  <button
                    type="button"
                    onClick={() => setSimpleType(null)}
                    className={cn(
                      "rounded-md border px-3 py-1 text-sm transition-colors",
                      !simpleType
                        ? "border-primary bg-primary/10 text-primary"
                        : "border-border/60 text-foreground/60 hover:bg-primary-hover",
                    )}
                  >
                    {t("smart.typeAny")}
                  </button>
                  {TYPE_ENUM.map((tp) => (
                    <button
                      key={tp}
                      type="button"
                      onClick={() => setSimpleType(tp)}
                      className={cn(
                        "rounded-md border px-3 py-1 text-sm transition-colors",
                        simpleType === tp
                          ? "border-primary bg-primary/10 text-primary"
                          : "border-border/60 text-foreground/60 hover:bg-primary-hover",
                      )}
                    >
                      {t(`smart.type_${tp}`)}
                    </button>
                  ))}
                </div>
              </div>

              {/* 预设开关 */}
              <div className="flex flex-col gap-1.5">
                <label className="text-xs font-medium text-foreground/70">{t("smart.presetLabel")}</label>
                <div className="flex flex-wrap gap-1.5">
                  {SIMPLE_PRESETS.map((preset) => {
                    const active = rules.some((r) => preset.matches(r));
                    return (
                      <button
                        key={preset.key}
                        type="button"
                        onClick={() => togglePreset(preset.key)}
                        className={cn(
                          "rounded-full border px-3 py-1 text-xs transition-colors",
                          active
                            ? "border-primary bg-primary/10 text-primary"
                            : "border-border/60 text-foreground/60 hover:bg-primary-hover",
                        )}
                      >
                        {t(preset.labelKey)}
                      </button>
                    );
                  })}
                </div>
              </div>
            </div>
          ) : (
            <div className="flex flex-col gap-3">
              {/* 组合子 */}
              <div className="flex items-center gap-2 text-sm">
                <span className="text-foreground/60">{t("smart.matchLabel")}</span>
                <div className="flex items-center gap-1 rounded-md bg-muted/50 p-0.5">
                  {(["and", "or"] as RuleCombinator[]).map((c) => (
                    <button
                      key={c}
                      type="button"
                      onClick={() => setCombinator(c)}
                      className={cn(
                        "rounded px-2.5 py-0.5 text-xs transition-colors",
                        combinator === c
                          ? "bg-background font-medium text-foreground shadow-sm"
                          : "text-foreground/55 hover:text-foreground",
                      )}
                    >
                      {c === "and" ? t("smart.combAnd") : t("smart.combOr")}
                    </button>
                  ))}
                </div>
              </div>

              {/* 规则行 */}
              <div className="flex flex-col gap-2">
                {rules.map((rule, idx) => (
                  <SmartRuleRow
                    key={idx}
                    rule={rule}
                    tags={tags}
                    onChange={(next) => updateRule(idx, next)}
                    onRemove={() => removeRule(idx)}
                  />
                ))}
                {rules.length === 0 && (
                  <p className="py-2 text-center text-xs text-foreground/40">{t("smart.noRules")}</p>
                )}
              </div>

              <Button variant="outline" size="sm" onClick={addRule} className="w-full">
                <Plus className="size-3.5" />
                {t("smart.addRule")}
              </Button>
            </div>
          )}
        </div>

        {/* 自然语言回显 + 命中数 */}
        <div className="flex flex-col gap-1.5 rounded-md border border-border/50 bg-muted/30 p-3">
          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-foreground/60">{t("smart.previewLabel")}</span>
            <span className="text-xs text-primary">
              {previewing ? (
                <Loader2 className="size-3 animate-spin" />
              ) : matchCount != null ? (
                t("smart.matchCount", { count: matchCount })
              ) : (
                ""
              )}
            </span>
          </div>
          <p className="text-xs leading-relaxed text-foreground/70">
            {validationErrKey ? t(validationErrKey) : nlText}
          </p>
        </div>

        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={saving}>
            {t("smart.cancel")}
          </Button>
          <Button onClick={handleSave} disabled={saving || !!validationErrKey || !name.trim()}>
            {saving && <Loader2 className="size-4 animate-spin" />}
            {t("smart.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};

export default SmartCollectionDialog;
