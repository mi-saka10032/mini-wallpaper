use garde::Validate;
use serde::Deserialize;

/// 已知的 setting key 白名单
const VALID_KEYS: &[&str] = &["pause_on_fullscreen", "global_volume"];

/// 设置键值对请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct SetSettingRequest {
    /// setting key：必须在白名单内
    #[garde(custom(validate_setting_key))]
    pub key: String,
    /// setting value：非空，且格式需匹配 key 的要求
    #[garde(custom(validate_setting_value_by_key))]
    pub value: String,
}

// ==================== 自定义校验函数 ====================

/// 校验 key 是否在白名单内
fn validate_setting_key(value: &String, _ctx: &()) -> garde::Result {
    if !VALID_KEYS.contains(&value.as_str()) {
        return Err(garde::Error::new(format!(
            "不支持的设置项 '{}', 仅支持: {}",
            value,
            VALID_KEYS.join(", ")
        )));
    }
    Ok(())
}

/// 校验 value 格式（此处仅做非空校验，跨字段校验在 Validated 反序列化后由 validate_cross_fields 处理）
fn validate_setting_value_by_key(value: &String, _ctx: &()) -> garde::Result {
    if value.is_empty() {
        return Err(garde::Error::new("value 不能为空"));
    }
    Ok(())
}

impl SetSettingRequest {
    /// 跨字段校验：按 key 校验 value 的格式
    pub fn validate_value_format(&self) -> Result<(), String> {
        match self.key.as_str() {
            "pause_on_fullscreen" => {
                if self.value != "true" && self.value != "false" {
                    return Err("pause_on_fullscreen 的值仅支持 true/false".to_string());
                }
            }
            "global_volume" => match self.value.parse::<u32>() {
                Ok(v) if v <= 100 => {}
                _ => {
                    return Err("global_volume 的值必须为 0~100 的整数".to_string());
                }
            },
            _ => {}
        }
        Ok(())
    }
}

/// 获取单个设置值请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct GetSettingRequest {
    /// setting key：必须在白名单内
    #[garde(custom(validate_setting_key))]
    pub key: String,
}