use garde::Validate;
use serde::Deserialize;

/// 导入壁纸请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ImportWallpapersRequest {
    /// 文件路径列表：非空
    #[garde(length(min = 1))]
    pub paths: Vec<String>,
}

/// 批量删除壁纸请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct DeleteWallpapersRequest {
    /// 壁纸 ID 列表：非空
    #[garde(length(min = 1))]
    pub ids: Vec<i32>,
}

/// 单个待导入文件（字节方式）
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ImportFileItem {
    /// 原始文件名（含扩展名）：非空
    #[garde(length(min = 1))]
    pub name: String,
    /// 文件字节内容：非空
    #[garde(length(min = 1))]
    pub bytes: Vec<u8>,
}

/// 通过字节方式导入壁纸请求（H5 拖拽场景）
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ImportWallpapersBytesRequest {
    /// 文件列表：非空
    #[garde(length(min = 1), dive)]
    pub items: Vec<ImportFileItem>,
}
