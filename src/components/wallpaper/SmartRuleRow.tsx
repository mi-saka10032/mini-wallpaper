import { Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { RuleField, RuleItem, TagWithCount } from "@/api/config";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { TagInput } from "@/components/wallpaper/TagInput";
import {
  FIELD_META,
  ORIENTATION_ENUM,
  TYPE_ENUM,
  defaultValueFor,
  getFieldMeta,
  getValueKind,
} from "@/lib/smartRuleMeta";

// ============ 类型定义 ============

export interface SmartRuleRowProps {
  rule: RuleItem;
  /** 全部标签（供 tagIds 值输入的名字↔id 转换与联想） */
  tags: TagWithCount[];
  onChange: (next: RuleItem) => void;
  onRemove: () => void;
}

/**
 * 高级档单条规则行编辑器
 *
 * 三段式：字段下拉 → 操作符下拉 → 值输入（随 valueKind 切换控件）。
 * 切换字段/操作符时若值类型变化，自动重置为该类型的默认值。
 */
export const SmartRuleRow: React.FC<SmartRuleRowProps> = ({ rule, tags, onChange, onRemove }) => {
  const { t } = useTranslation();

  const fieldMeta = getFieldMeta(rule.field);
  const valueKind = getValueKind(rule.field, rule.op);

  // 切换字段：默认取新字段的第一个操作符，并重置值
  const handleFieldChange = (field: RuleField) => {
    const meta = getFieldMeta(field);
    const op = meta?.ops[0];
    if (!op) return;
    onChange({ field, op: op.op, value: defaultValueFor(op.valueKind) });
  };

  // 切换操作符：若值类型变化则重置值
  const handleOpChange = (op: string) => {
    const newKind = getValueKind(rule.field, op);
    const oldKind = getValueKind(rule.field, rule.op);
    onChange({
      ...rule,
      op,
      value: newKind === oldKind ? rule.value : defaultValueFor(newKind ?? "number"),
    });
  };

  // tagIds 值输入：id 数组 ↔ 名字数组
  const tagNames = (() => {
    const ids = Array.isArray(rule.value) ? (rule.value as number[]) : [];
    const nameById = new Map(tags.map((tenta) => [tenta.id, tenta.name]));
    return ids.map((id) => nameById.get(id)).filter((n): n is string => n != null);
  })();

  const handleTagNamesChange = (names: string[]) => {
    // 名字 → id：仅保留已存在的标签（不存在的忽略，保存前无 id）
    // 允许输入新标签名：用负数占位 id 不可行（后端按 id 求值），故规则里标签必须已存在。
    // 这里策略：把不存在的名字也保留为「待创建」——但规则存 id，故仅收已存在 id。
    const idByName = new Map(tags.map((tenta) => [tenta.name, tenta.id]));
    const ids = names
      .map((n) => idByName.get(n))
      .filter((id): id is number => id != null);
    onChange({ ...rule, value: ids });
  };

  const renderValueInput = () => {
    switch (valueKind) {
      case "tagIds":
        return (
          <TagInput
            value={tagNames}
            onChange={handleTagNamesChange}
            suggestions={tags.map((tenta) => tenta.name)}
            placeholder={t("smart.tagValuePlaceholder")}
            className="flex-1"
          />
        );
      case "typeEnum":
        return (
          <Select value={String(rule.value)} onValueChange={(v) => onChange({ ...rule, value: v })}>
            <SelectTrigger size="sm" className="h-8 flex-1">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {TYPE_ENUM.map((v) => (
                <SelectItem key={v} value={v}>
                  {t(`smart.type_${v}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        );
      case "orientationEnum":
        return (
          <Select value={String(rule.value)} onValueChange={(v) => onChange({ ...rule, value: v })}>
            <SelectTrigger size="sm" className="h-8 flex-1">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {ORIENTATION_ENUM.map((v) => (
                <SelectItem key={v} value={v}>
                  {t(`smart.orientation_${v}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        );
      case "number":
      case "days":
        return (
          <Input
            type="number"
            value={Number(rule.value)}
            min={valueKind === "days" ? 1 : 0}
            onChange={(e) => onChange({ ...rule, value: Number(e.target.value) })}
            className="h-8 flex-1"
          />
        );
      case "numberRange": {
        const arr = Array.isArray(rule.value) ? (rule.value as number[]) : [0, 0];
        return (
          <div className="flex flex-1 items-center gap-1.5">
            <Input
              type="number"
              value={arr[0]}
              min={0}
              onChange={(e) => onChange({ ...rule, value: [Number(e.target.value), arr[1]] })}
              className="h-8 w-full"
            />
            <span className="text-foreground/40">~</span>
            <Input
              type="number"
              value={arr[1]}
              min={0}
              onChange={(e) => onChange({ ...rule, value: [arr[0], Number(e.target.value)] })}
              className="h-8 w-full"
            />
          </div>
        );
      }
      case "date":
        return (
          <Input
            type="date"
            value={String(rule.value)}
            onChange={(e) => onChange({ ...rule, value: e.target.value })}
            className="h-8 flex-1"
          />
        );
      default:
        return null;
    }
  };

  return (
    <div className="flex items-start gap-1.5">
      {/* 字段 */}
      <Select value={rule.field} onValueChange={(v) => handleFieldChange(v as RuleField)}>
        <SelectTrigger size="sm" className="h-8 w-28 shrink-0">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {FIELD_META.map((f) => (
            <SelectItem key={f.field} value={f.field}>
              {t(f.labelKey)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      {/* 操作符 */}
      <Select value={rule.op} onValueChange={handleOpChange}>
        <SelectTrigger size="sm" className="h-8 w-24 shrink-0">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {fieldMeta?.ops.map((o) => (
            <SelectItem key={o.op} value={o.op}>
              {t(o.labelKey)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      {/* 值 */}
      {renderValueInput()}

      {/* 删除该行 */}
      <button
        type="button"
        onClick={onRemove}
        className="flex size-8 shrink-0 items-center justify-center rounded-md text-foreground/40 transition-colors hover:bg-destructive/10 hover:text-destructive"
        title={t("smart.removeRule")}
      >
        <Trash2 className="size-3.5" />
      </button>
    </div>
  );
};

export default SmartRuleRow;
