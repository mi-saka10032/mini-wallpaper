use garde::Validate;
use serde::Deserialize;

/// 切换壁纸方向
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Next,
    Prev,
}

/// 切换壁纸请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct SwitchWallpaperRequest {
    /// 切换方向
    #[garde(skip)]
    pub direction: Direction,
}
