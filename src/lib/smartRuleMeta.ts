import type { RuleField, RuleItem, SmartRule } from "@/api/config";

/**
 * 智能收藏夹规则元数据（前端唯一真相源）
 *
 * 与后端 `smart_rule.rs` 白名单严格对齐：字段 → 允许的操作符 → 值类型。
 * 任何超出此表的组合，后端都会拒绝保存，故前端构建器只允许生成合法组合。
 */

/** 值输入形态 */
export type ValueKind =
  | "tagIds" // 标签 id 数组
  | "typeEnum" // image / video / gif
  | "orientationEnum" // landscape / portrait / square
  | "number" // 单个整数
  | "numberRange" // [min, max]
  | "days" // 正整数天数
  | "date"; // YYYY-MM-DD

export interface OpMeta {
  op: string;
  /** i18n key（op 文案） */
  labelKey: string;
  valueKind: ValueKind;
}

export interface FieldMeta {
  field: RuleField;
  /** i18n key（字段名） */
  labelKey: string;
  ops: OpMeta[];
}

/** 字段 → 操作符 → 值类型 白名单（与后端一一对应） */
export const FIELD_META: FieldMeta[] = [
  {
    field: "tag",
    labelKey: "smart.fieldTag",
    ops: [
      { op: "includes_any", labelKey: "smart.opIncludesAny", valueKind: "tagIds" },
      { op: "includes_all", labelKey: "smart.opIncludesAll", valueKind: "tagIds" },
      { op: "excludes", labelKey: "smart.opExcludes", valueKind: "tagIds" },
    ],
  },
  {
    field: "type",
    labelKey: "smart.fieldType",
    ops: [
      { op: "eq", labelKey: "smart.opEq", valueKind: "typeEnum" },
      { op: "neq", labelKey: "smart.opNeq", valueKind: "typeEnum" },
    ],
  },
  {
    field: "width",
    labelKey: "smart.fieldWidth",
    ops: [
      { op: "gte", labelKey: "smart.opGte", valueKind: "number" },
      { op: "lte", labelKey: "smart.opLte", valueKind: "number" },
      { op: "between", labelKey: "smart.opBetween", valueKind: "numberRange" },
    ],
  },
  {
    field: "height",
    labelKey: "smart.fieldHeight",
    ops: [
      { op: "gte", labelKey: "smart.opGte", valueKind: "number" },
      { op: "lte", labelKey: "smart.opLte", valueKind: "number" },
      { op: "between", labelKey: "smart.opBetween", valueKind: "numberRange" },
    ],
  },
  {
    field: "orientation",
    labelKey: "smart.fieldOrientation",
    ops: [{ op: "eq", labelKey: "smart.opEq", valueKind: "orientationEnum" }],
  },
  {
    field: "created_at",
    labelKey: "smart.fieldCreatedAt",
    ops: [
      { op: "within_days", labelKey: "smart.opWithinDays", valueKind: "days" },
      { op: "before", labelKey: "smart.opBefore", valueKind: "date" },
      { op: "after", labelKey: "smart.opAfter", valueKind: "date" },
    ],
  },
  {
    field: "file_size",
    labelKey: "smart.fieldFileSize",
    ops: [
      { op: "gte", labelKey: "smart.opGte", valueKind: "number" },
      { op: "lte", labelKey: "smart.opLte", valueKind: "number" },
    ],
  },
];

/** 枚举取值 */
export const TYPE_ENUM = ["image", "video", "gif"] as const;
export const ORIENTATION_ENUM = ["landscape", "portrait", "square"] as const;

/** 查字段元数据 */
export function getFieldMeta(field: RuleField): FieldMeta | undefined {
  return FIELD_META.find((f) => f.field === field);
}

/** 查某字段某操作符的值类型 */
export function getValueKind(field: RuleField, op: string): ValueKind | undefined {
  return getFieldMeta(field)?.ops.find((o) => o.op === op)?.valueKind;
}

/** 为「字段 + 操作符」构造该值类型的空白初值 */
export function defaultValueFor(kind: ValueKind): unknown {
  switch (kind) {
    case "tagIds":
      return [];
    case "typeEnum":
      return "image";
    case "orientationEnum":
      return "landscape";
    case "number":
      return 0;
    case "numberRange":
      return [0, 0];
    case "days":
      return 7;
    case "date":
      return new Date().toISOString().slice(0, 10);
    default:
      return null;
  }
}

/** 新建一条默认规则（默认第一个字段 + 其第一个操作符） */
export function newDefaultRule(): RuleItem {
  const field = FIELD_META[0];
  const op = field.ops[0];
  return {
    field: field.field,
    op: op.op,
    value: defaultValueFor(op.valueKind),
  };
}

// ==================== 简单档预设 ====================

/** 简单档预设开关：本质是往 rules 追加一条固定条件 */
export interface SimplePreset {
  key: string;
  labelKey: string;
  build: () => RuleItem;
  /** 判断一条规则是否等价于本预设（用于回填开关态） */
  matches: (r: RuleItem) => boolean;
}

export const SIMPLE_PRESETS: SimplePreset[] = [
  {
    key: "landscapeOnly",
    labelKey: "smart.presetLandscape",
    build: () => ({ field: "orientation", op: "eq", value: "landscape" }),
    matches: (r) => r.field === "orientation" && r.op === "eq" && r.value === "landscape",
  },
  {
    key: "ultraHd",
    labelKey: "smart.preset4k",
    build: () => ({ field: "width", op: "gte", value: 3840 }),
    matches: (r) => r.field === "width" && r.op === "gte" && r.value === 3840,
  },
  {
    key: "recent7d",
    labelKey: "smart.presetRecent7d",
    build: () => ({ field: "created_at", op: "within_days", value: 7 }),
    matches: (r) => r.field === "created_at" && r.op === "within_days" && r.value === 7,
  },
];

// ==================== 自然语言回显 ====================

export interface NlContext {
  t: (key: string, opts?: Record<string, unknown>) => string;
  /** tag id → name 映射，用于把标签条件回显为名字 */
  tagNameById: Map<number, string>;
}

/** 单条规则 → 自然语言短语 */
export function ruleToText(rule: RuleItem, ctx: NlContext): string {
  const { t, tagNameById } = ctx;
  const fieldLabel = t(getFieldMeta(rule.field)?.labelKey ?? rule.field);
  const opMeta = getFieldMeta(rule.field)?.ops.find((o) => o.op === rule.op);
  const opLabel = t(opMeta?.labelKey ?? rule.op);

  let valueText = "";
  const kind = opMeta?.valueKind;
  switch (kind) {
    case "tagIds": {
      const ids = Array.isArray(rule.value) ? (rule.value as number[]) : [];
      valueText = ids.map((id) => tagNameById.get(id) ?? `#${id}`).join("、");
      break;
    }
    case "typeEnum":
      valueText = t(`smart.type_${String(rule.value)}`);
      break;
    case "orientationEnum":
      valueText = t(`smart.orientation_${String(rule.value)}`);
      break;
    case "numberRange": {
      const arr = Array.isArray(rule.value) ? (rule.value as number[]) : [0, 0];
      valueText = `${arr[0]} ~ ${arr[1]}`;
      break;
    }
    case "days":
      valueText = t("smart.daysValue", { count: Number(rule.value) });
      break;
    default:
      valueText = String(rule.value);
  }

  return `${fieldLabel} ${opLabel} ${valueText}`.trim();
}

/** 整个规则 → 自然语言（按组合子连接） */
export function smartRuleToText(rule: SmartRule, ctx: NlContext): string {
  if (!rule.rules.length) return ctx.t("smart.noConditions");
  const joiner = rule.combinator === "and" ? ctx.t("smart.joinAnd") : ctx.t("smart.joinOr");
  return rule.rules.map((r) => ruleToText(r, ctx)).join(` ${joiner} `);
}

/** 校验：至少一条规则 + 标签条件非空 */
export function validateSmartRule(rule: SmartRule): string | null {
  if (!rule.rules.length) return "smart.errEmpty";
  for (const r of rule.rules) {
    const kind = getValueKind(r.field, r.op);
    if (!kind) return "smart.errInvalidOp";
    if (kind === "tagIds") {
      const ids = Array.isArray(r.value) ? (r.value as number[]) : [];
      if (ids.length === 0) return "smart.errTagEmpty";
    }
    if (kind === "numberRange") {
      const arr = Array.isArray(r.value) ? (r.value as number[]) : [];
      if (arr.length !== 2 || arr[0] > arr[1]) return "smart.errRange";
    }
  }
  return null;
}
