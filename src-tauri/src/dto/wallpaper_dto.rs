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
