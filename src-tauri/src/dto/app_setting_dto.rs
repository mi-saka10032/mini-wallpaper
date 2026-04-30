use garde::Validate;
use serde::Deserialize;

// ==================== Setting Keys（Single Source of Truth）====================

/// 已知的 setting key 常量（与前端 SETTING_KEYS 一一对应）
pub mod keys {
    pub const THEME: &str = "theme";
    pub const LANGUAGE: &str = "language";
    pub const CLOSE_TO_TRAY: &str = "close_to_tray";
    pub const PAUSE_ON_FULLSCREEN: &str = "pause_on_fullscreen";
    pub const GLOBAL_VOLUME: &str = "global_volume";
    pub const DISPLAY_MODE: &str = "display_mode";
    pub const SHORTCUT_NEXT_WALLPAPER: &str = "shortcut_next_wallpaper";
    pub const SHORTCUT_PREV_WALLPAPER: &str = "shortcut_prev_wallpaper";
    pub const ACCENT_COLOR: &str = "accent_color";
}

/// 已知的 setting key 白名单（由 keys 模块常量自动组成）
const VALID_KEYS: &[&str] = &[
    keys::THEME,
    keys::LANGUAGE,
    keys::CLOSE_TO_TRAY,
    keys::PAUSE_ON_FULLSCREEN,
    keys::GLOBAL_VOLUME,
    keys::DISPLAY_MODE,
    keys::SHORTCUT_NEXT_WALLPAPER,
    keys::SHORTCUT_PREV_WALLPAPER,
    keys::ACCENT_COLOR,
];

/// 允许的 display_mode 枚举值
const VALID_DISPLAY_MODES: &[&str] = &["independent", "mirror", "extend"];


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

/// 校验 value 格式
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
            "pause_on_fullscreen" | "close_to_tray" => {
                if self.value != "true" && self.value != "false" {
                    return Err(format!("{} 的值仅支持 true/false", self.key));
                }
            }
            "global_volume" => match self.value.parse::<u32>() {
                Ok(v) if v <= 100 => {}
                _ => {
                    return Err("global_volume 的值必须为 0~100 的整数".to_string());
                }
            },
            "display_mode" => {
                if !VALID_DISPLAY_MODES.contains(&self.value.as_str()) {
                    return Err(format!(
                        "display_mode 仅支持 {}",
                        VALID_DISPLAY_MODES.join("/")
                    ));
                }
            }
            // theme, language, shortcut_* 等仅需非空校验（已由 garde 保证）
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