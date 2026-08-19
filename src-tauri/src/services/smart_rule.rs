//! 智能收藏夹规则引擎
//!
//! 把「结构化 JSON 规则」编译为对 `wallpapers` 表的参数化查询条件（SeaORM
//! `Condition`），命中集实时求值、不物化。全链路遵循：
//!
//! ```text
//! JSON 反序列化 → 字段/操作符/值类型白名单校验 → SeaORM Condition（绑定参数）
//! ```
//!
//! ## 安全底线
//!
//! - **绝不存 / 不解释裸类 SQL**：规则以强类型 JSON 存储与传输。
//! - **白名单唯一真相源**：`field` / `op` / 值类型任一不在白名单即整条规则判非法拒绝。
//! - **全参数化**：所有 value 经 SeaORM 走绑定参数，杜绝注入。
//! - **标签按 id**：标签条件的 value 是 tag id 数组（非 name 字符串），
//!   查询 join `wallpaper_tags` 子查询求值；标签改名不影响规则。
//!
//! ## 顶层结构
//!
//! ```json
//! {
//!   "version": 1,
//!   "combinator": "and",           // and | or
//!   "rules": [
//!     { "field": "tag",    "op": "includes_any", "value": [1, 2] },
//!     { "field": "width",  "op": "gte",          "value": 3840 },
//!     { "field": "type",   "op": "eq",           "value": "image" }
//!   ]
//! }
//! ```
//!
//! 硬约束：`rules.length >= 1`（空规则等于匹配全部，拒绝保存）。

use anyhow::{bail, Result};
use sea_orm::prelude::Expr;
use sea_orm::sea_query::Query;
use sea_orm::{ColumnTrait, Condition};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::entities::{wallpaper, wallpaper_tag};

/// 组合子：规则之间的逻辑关系
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Combinator {
    And,
    Or,
}

/// 单条规则：字段 + 操作符 + 值（值为透传 JSON，按 field+op 再做类型校验）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleItem {
    pub field: String,
    pub op: String,
    pub value: JsonValue,
}

/// 智能收藏夹规则（顶层 JSON schema）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartRule {
    /// 版本号（预留向后兼容）
    #[serde(default = "default_version")]
    pub version: u32,
    pub combinator: Combinator,
    pub rules: Vec<RuleItem>,
}

fn default_version() -> u32 {
    1
}

impl SmartRule {
    /// 从 JSON 字符串解析并做完整白名单校验
    pub fn parse_and_validate(json: &str) -> Result<Self> {
        let rule: SmartRule = serde_json::from_str(json)
            .map_err(|e| anyhow::anyhow!("规则 JSON 解析失败: {}", e))?;
        rule.validate()?;
        Ok(rule)
    }

    /// 序列化为紧凑 JSON（入库用）
    #[allow(dead_code)]
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    /// 白名单 + 结构校验
    pub fn validate(&self) -> Result<()> {
        // 硬约束：至少一条规则（空规则等于匹配全部，拒绝）
        if self.rules.is_empty() {
            bail!("智能收藏夹至少需要一条筛选规则");
        }
        for (i, item) in self.rules.iter().enumerate() {
            item.validate()
                .map_err(|e| anyhow::anyhow!("第 {} 条规则非法: {}", i + 1, e))?;
        }
        Ok(())
    }

    /// 编译为 SeaORM 查询条件（参数化）
    ///
    /// 组合子决定 `all()`(AND) / `any()`(OR)；每条规则编译为一个子条件后加入。
    pub fn build_condition(&self) -> Result<Condition> {
        let mut cond = match self.combinator {
            Combinator::And => Condition::all(),
            Combinator::Or => Condition::any(),
        };
        for item in &self.rules {
            cond = cond.add(item.build_condition()?);
        }
        Ok(cond)
    }
}

/// 白名单字段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Tag,
    Type,
    Width,
    Height,
    Orientation,
    CreatedAt,
    FileSize,
}

impl Field {
    fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "tag" => Field::Tag,
            "type" => Field::Type,
            "width" => Field::Width,
            "height" => Field::Height,
            "orientation" => Field::Orientation,
            "created_at" => Field::CreatedAt,
            "file_size" => Field::FileSize,
            other => bail!("未知字段 '{}'（不在白名单）", other),
        })
    }
}

impl RuleItem {
    /// 单条规则的白名单校验（field → 允许的 op → 值类型）
    fn validate(&self) -> Result<()> {
        // 借助 build_condition 内部的完整校验，避免逻辑重复
        self.build_condition().map(|_| ())
    }

    /// 编译单条规则为查询条件（含 field/op/值类型白名单校验 + 参数绑定）
    fn build_condition(&self) -> Result<Condition> {
        let field = Field::parse(&self.field)?;
        match field {
            Field::Tag => self.build_tag_condition(),
            Field::Type => self.build_type_condition(),
            Field::Width => self.build_num_condition(wallpaper::Column::Width),
            Field::Height => self.build_num_condition(wallpaper::Column::Height),
            Field::Orientation => self.build_orientation_condition(),
            Field::CreatedAt => self.build_created_at_condition(),
            Field::FileSize => self.build_file_size_condition(),
        }
    }

    // ---------- tag：includes_any / includes_all / excludes（value = tag id 数组）----------

    fn build_tag_condition(&self) -> Result<Condition> {
        let ids = self.value_as_i32_array()?;
        if ids.is_empty() {
            bail!("标签条件的标签列表不能为空");
        }

        // 复用：某壁纸命中「至少一个给定 tag id」的存在性子查询
        let in_any = wallpaper::Column::Id.in_subquery(
            Query::select()
                .column(wallpaper_tag::Column::WallpaperId)
                .from(wallpaper_tag::Entity)
                .and_where(wallpaper_tag::Column::TagId.is_in(ids.clone()))
                .to_owned(),
        );

        match self.op.as_str() {
            "includes_any" => Ok(Condition::all().add(in_any)),
            "excludes" => Ok(Condition::all().add(in_any.not())),
            "includes_all" => {
                // 必须同时含全部 tag：对每个 id 各加一个存在性子查询（AND）
                let mut cond = Condition::all();
                for id in ids {
                    let has_one = wallpaper::Column::Id.in_subquery(
                        Query::select()
                            .column(wallpaper_tag::Column::WallpaperId)
                            .from(wallpaper_tag::Entity)
                            .and_where(wallpaper_tag::Column::TagId.eq(id))
                            .to_owned(),
                    );
                    cond = cond.add(has_one);
                }
                Ok(cond)
            }
            other => bail!("字段 tag 不支持操作符 '{}'", other),
        }
    }

    // ---------- type：eq / neq（value = 枚举 image/video/gif）----------

    fn build_type_condition(&self) -> Result<Condition> {
        let v = self.value_as_enum(&["image", "video", "gif"])?;
        match self.op.as_str() {
            "eq" => Ok(Condition::all().add(wallpaper::Column::Type.eq(v))),
            "neq" => Ok(Condition::all().add(wallpaper::Column::Type.ne(v))),
            other => bail!("字段 type 不支持操作符 '{}'", other),
        }
    }

    // ---------- width / height：gte / lte / between（value = 数字或 [min,max]）----------

    fn build_num_condition(&self, col: wallpaper::Column) -> Result<Condition> {
        match self.op.as_str() {
            "gte" => {
                let n = self.value_as_i64()?;
                Ok(Condition::all().add(col.gte(n)))
            }
            "lte" => {
                let n = self.value_as_i64()?;
                Ok(Condition::all().add(col.lte(n)))
            }
            "between" => {
                let (min, max) = self.value_as_i64_pair()?;
                Ok(Condition::all().add(col.between(min, max)))
            }
            other => bail!("数值字段不支持操作符 '{}'", other),
        }
    }

    // ---------- orientation：eq（由 width/height 派生）----------

    fn build_orientation_condition(&self) -> Result<Condition> {
        if self.op != "eq" {
            bail!("字段 orientation 仅支持操作符 eq");
        }
        let v = self.value_as_enum(&["landscape", "portrait", "square"])?;
        let w = Expr::col(wallpaper::Column::Width);
        let h = Expr::col(wallpaper::Column::Height);
        let cond = match v.as_str() {
            "landscape" => Condition::all().add(w.gt(h)),
            "portrait" => Condition::all().add(Expr::col(wallpaper::Column::Width).lt(Expr::col(wallpaper::Column::Height))),
            "square" => Condition::all().add(Expr::col(wallpaper::Column::Width).eq(Expr::col(wallpaper::Column::Height))),
            _ => unreachable!(),
        };
        Ok(cond)
    }

    // ---------- created_at：within_days / before / after ----------

    fn build_created_at_condition(&self) -> Result<Condition> {
        match self.op.as_str() {
            "within_days" => {
                let days = self.value_as_i64()?;
                if days <= 0 {
                    bail!("within_days 必须为正整数");
                }
                let threshold = chrono::Local::now() - chrono::Duration::days(days);
                let threshold_str = threshold.format("%Y-%m-%d %H:%M:%S").to_string();
                Ok(Condition::all().add(wallpaper::Column::CreatedAt.gte(threshold_str)))
            }
            "before" => {
                let date = self.value_as_date_str()?;
                Ok(Condition::all().add(wallpaper::Column::CreatedAt.lt(date)))
            }
            "after" => {
                let date = self.value_as_date_str()?;
                Ok(Condition::all().add(wallpaper::Column::CreatedAt.gt(date)))
            }
            other => bail!("字段 created_at 不支持操作符 '{}'", other),
        }
    }

    // ---------- file_size：gte / lte（字节数）----------

    fn build_file_size_condition(&self) -> Result<Condition> {
        match self.op.as_str() {
            "gte" => {
                let n = self.value_as_i64()?;
                Ok(Condition::all().add(wallpaper::Column::FileSize.gte(n)))
            }
            "lte" => {
                let n = self.value_as_i64()?;
                Ok(Condition::all().add(wallpaper::Column::FileSize.lte(n)))
            }
            other => bail!("字段 file_size 不支持操作符 '{}'", other),
        }
    }

    // ==================== 值类型提取与校验 ====================

    fn value_as_i32_array(&self) -> Result<Vec<i32>> {
        let arr = self
            .value
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("值应为数组"))?;
        let mut out = Vec::with_capacity(arr.len());
        for v in arr {
            let n = v
                .as_i64()
                .ok_or_else(|| anyhow::anyhow!("数组元素应为整数"))?;
            out.push(i32::try_from(n).map_err(|_| anyhow::anyhow!("整数超出范围"))?);
        }
        Ok(out)
    }

    fn value_as_i64(&self) -> Result<i64> {
        self.value
            .as_i64()
            .ok_or_else(|| anyhow::anyhow!("值应为整数"))
    }

    fn value_as_i64_pair(&self) -> Result<(i64, i64)> {
        let arr = self
            .value
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("between 的值应为 [min, max] 数组"))?;
        if arr.len() != 2 {
            bail!("between 的值应恰好包含 2 个元素");
        }
        let min = arr[0].as_i64().ok_or_else(|| anyhow::anyhow!("min 应为整数"))?;
        let max = arr[1].as_i64().ok_or_else(|| anyhow::anyhow!("max 应为整数"))?;
        if min > max {
            bail!("between 的 min 不能大于 max");
        }
        Ok((min, max))
    }

    fn value_as_enum(&self, allowed: &[&str]) -> Result<String> {
        let s = self
            .value
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("值应为字符串"))?;
        if !allowed.contains(&s) {
            bail!("值 '{}' 不在允许集合 {:?} 内", s, allowed);
        }
        Ok(s.to_string())
    }

    fn value_as_date_str(&self) -> Result<String> {
        let s = self
            .value
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("日期值应为字符串"))?;
        // 校验格式 YYYY-MM-DD，避免脏字符串进入查询（虽已参数化，仍做语义校验）
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map_err(|_| anyhow::anyhow!("日期格式应为 YYYY-MM-DD"))?;
        Ok(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_rules() {
        let json = r#"{"combinator":"and","rules":[]}"#;
        assert!(SmartRule::parse_and_validate(json).is_err());
    }

    #[test]
    fn rejects_unknown_field() {
        let json = r#"{"combinator":"and","rules":[{"field":"evil","op":"eq","value":1}]}"#;
        assert!(SmartRule::parse_and_validate(json).is_err());
    }

    #[test]
    fn rejects_bad_op_for_field() {
        let json = r#"{"combinator":"and","rules":[{"field":"type","op":"gte","value":"image"}]}"#;
        assert!(SmartRule::parse_and_validate(json).is_err());
    }

    #[test]
    fn accepts_valid_composite_rule() {
        let json = r#"{
            "version":1,
            "combinator":"and",
            "rules":[
                {"field":"tag","op":"includes_any","value":[1,2]},
                {"field":"width","op":"gte","value":3840},
                {"field":"type","op":"eq","value":"image"},
                {"field":"created_at","op":"within_days","value":7},
                {"field":"file_size","op":"lte","value":10485760},
                {"field":"orientation","op":"eq","value":"landscape"}
            ]
        }"#;
        let rule = SmartRule::parse_and_validate(json).expect("should be valid");
        assert!(rule.build_condition().is_ok());
    }

    #[test]
    fn rejects_between_reversed() {
        let json = r#"{"combinator":"and","rules":[{"field":"width","op":"between","value":[3840,1920]}]}"#;
        assert!(SmartRule::parse_and_validate(json).is_err());
    }
}