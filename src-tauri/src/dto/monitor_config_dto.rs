use garde::Validate;
use serde::Deserialize;

/// 允许的 fit_mode 枚举值
const VALID_FIT_MODES: &[&str] = &["cover", "contain", "fill", "stretch", "center"];
/// 允许的 play_mode 枚举值
const VALID_PLAY_MODES: &[&str] = &["sequential", "random"];

/// 创建或更新显示器配置请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct UpsertMonitorConfigRequest {
    /// 显示器标识：非空字符串
    #[garde(length(min = 1))]
    pub monitor_id: String,
    /// 固定壁纸 ID
    #[garde(skip)]
    pub wallpaper_id: Option<i32>,
    /// 关联收藏夹 ID
    #[garde(skip)]
    pub collection_id: Option<i32>,
    /// 是否显式清空 collection_id
    #[garde(skip)]
    pub clear_collection: Option<bool>,
    /// 壁纸适配模式：cover / contain / fill / stretch / center
    #[garde(custom(validate_fit_mode))]
    pub fit_mode: Option<String>,
    /// 播放模式：sequential / random
    #[garde(custom(validate_play_mode))]
    pub play_mode: Option<String>,
    /// 轮播间隔（秒）：10~86400
    #[garde(custom(validate_play_interval))]
    pub play_interval: Option<i32>,
    /// 是否启用轮播
    #[garde(skip)]
    pub is_enabled: Option<bool>,
    /// 显示器是否当前物理可用
    #[garde(skip)]
    pub active: Option<bool>,
}

/// 删除显示器配置请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct DeleteMonitorConfigRequest {
    /// 配置 ID：正整数
    #[garde(range(min = 1))]
    pub id: i32,
    /// 显示器 ID（用于停止定时器）
    #[garde(skip)]
    pub monitor_id: Option<String>,
}

/// 获取单个显示器配置请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct GetMonitorConfigRequest {
    /// 显示器标识：非空字符串
    #[garde(length(min = 1))]
    pub monitor_id: String,
}

// ==================== 自定义校验函数 ====================

/// 校验 fit_mode 枚举白名单
fn validate_fit_mode(value: &Option<String>, _ctx: &()) -> garde::Result {
    if let Some(v) = value {
        if !VALID_FIT_MODES.contains(&v.as_str()) {
            return Err(garde::Error::new(format!(
                "fit_mode 仅支持 {}",
                VALID_FIT_MODES.join("/")
            )));
        }
    }
    Ok(())
}

/// 校验 play_mode 枚举白名单
fn validate_play_mode(value: &Option<String>, _ctx: &()) -> garde::Result {
    if let Some(v) = value {
        if !VALID_PLAY_MODES.contains(&v.as_str()) {
            return Err(garde::Error::new(format!(
                "play_mode 仅支持 {}",
                VALID_PLAY_MODES.join("/")
            )));
        }
    }
    Ok(())
}

/// 校验 play_interval 范围：10~86400 秒
fn validate_play_interval(value: &Option<i32>, _ctx: &()) -> garde::Result {
    if let Some(v) = value {
        if *v < 10 || *v > 86400 {
            return Err(garde::Error::new(
                "play_interval 范围为 10~86400 秒",
            ));
        }
    }
    Ok(())
}