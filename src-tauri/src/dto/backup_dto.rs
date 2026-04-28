use garde::Validate;
use serde::Deserialize;

/// 导出备份请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ExportBackupRequest {
    /// 输出文件路径
    #[garde(length(min = 1))]
    pub output_path: String,
}

/// 导入备份请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ImportBackupRequest {
    /// zip 文件路径
    #[garde(length(min = 1))]
    pub zip_path: String,
}
