//! 壁纸窗口管理命令的请求 DTO
//!
//! 字段对应前端传入的显示器几何信息。渲染路由收口后，窗口 backend 的选择
//! 完全由后端协调器按壁纸 capability 决定，DTO 只保留 `extra_query` 做前端兼容。

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
    /// 额外查询参数（前端兼容保留，渲染路由收口后后端不再消费）
    #[garde(skip)]
    #[allow(dead_code)]
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
