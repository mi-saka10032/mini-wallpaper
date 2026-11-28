use garde::Validate;
use serde::Deserialize;

/// opener 选中的目录绝对路径
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct AddWatchedFolderRequest {
    #[garde(length(chars, min = 1))]
    pub path: String,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct RemoveWatchedFolderRequest {
    #[garde(range(min = 1))]
    pub id: i32,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct SyncWatchedFolderRequest {
    #[garde(range(min = 1))]
    pub collection_id: i32,
}

/// 主窗口当前 active 夹 id（可能为 0 或负值视图），不做范围校验
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct SetActiveViewRequest {
    #[garde(skip)]
    pub active_id: i32,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct DeleteWatchedWallpapersRequest {
    #[garde(length(min = 1))]
    pub ids: Vec<i32>,
}
