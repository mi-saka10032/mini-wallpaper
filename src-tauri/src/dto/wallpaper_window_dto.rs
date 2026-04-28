use garde::Validate;
use serde::Deserialize;

/// 创建壁纸窗口请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct CreateWallpaperWindowRequest {
    /// 显示器 ID
    #[garde(length(min = 1))]
    pub monitor_id: String,
    /// 窗口 x 坐标
    #[garde(skip)]
    pub x: i32,
    /// 窗口 y 坐标
    #[garde(skip)]
    pub y: i32,
    /// 窗口宽度
    #[garde(range(min = 1))]
    pub width: u32,
    /// 窗口高度
    #[garde(range(min = 1))]
    pub height: u32,
    /// 额外查询参数
    #[garde(skip)]
    pub extra_query: Option<String>,
}

/// 销毁壁纸窗口请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct DestroyWallpaperWindowRequest {
    /// 显示器 ID
    #[garde(length(min = 1))]
    pub monitor_id: String,
}
