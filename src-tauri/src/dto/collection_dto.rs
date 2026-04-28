use garde::Validate;
use serde::Deserialize;

/// 创建收藏夹请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct CreateCollectionRequest {
    /// 收藏夹名称：trim 后 1~32 个字符
    #[garde(length(chars, min = 1, max = 32))]
    pub name: String,
}

/// 重命名收藏夹请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct RenameCollectionRequest {
    /// 收藏夹 ID：正整数
    #[garde(range(min = 1))]
    pub id: i32,
    /// 收藏夹名称：trim 后 1~32 个字符
    #[garde(length(chars, min = 1, max = 32))]
    pub name: String,
}

/// 删除收藏夹请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct DeleteCollectionRequest {
    /// 收藏夹 ID：正整数
    #[garde(range(min = 1))]
    pub id: i32,
}

/// 获取收藏夹壁纸请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct GetCollectionWallpapersRequest {
    /// 收藏夹 ID：正整数
    #[garde(range(min = 1))]
    pub collection_id: i32,
}

/// 向收藏夹添加壁纸请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct AddWallpapersRequest {
    /// 收藏夹 ID：正整数
    #[garde(range(min = 1))]
    pub collection_id: i32,
    /// 壁纸 ID 列表：非空
    #[garde(length(min = 1))]
    pub wallpaper_ids: Vec<i32>,
}

/// 从收藏夹移除壁纸请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct RemoveWallpapersRequest {
    /// 收藏夹 ID：正整数
    #[garde(range(min = 1))]
    pub collection_id: i32,
    /// 壁纸 ID 列表：非空
    #[garde(length(min = 1))]
    pub wallpaper_ids: Vec<i32>,
}

/// 重新排序收藏夹内壁纸请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ReorderWallpapersRequest {
    /// 收藏夹 ID：正整数
    #[garde(range(min = 1))]
    pub collection_id: i32,
    /// 按新顺序排列的壁纸 ID 列表：非空
    #[garde(length(min = 1))]
    pub wallpaper_ids: Vec<i32>,
}
