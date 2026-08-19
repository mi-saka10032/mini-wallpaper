use garde::Validate;
use serde::Deserialize;

/// 给一批壁纸打一批标签请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct TagWallpapersRequest {
    /// 壁纸 ID 列表：非空
    #[garde(length(min = 1))]
    pub wallpaper_ids: Vec<i32>,
    /// 标签名列表：非空，每个 trim 后 1~16 字符
    #[garde(length(min = 1), inner(length(chars, min = 1, max = 16)))]
    pub tag_names: Vec<String>,
}

/// 从一批壁纸移除一批标签请求（按 tag id）
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct UntagWallpapersRequest {
    /// 壁纸 ID 列表：非空
    #[garde(length(min = 1))]
    pub wallpaper_ids: Vec<i32>,
    /// 标签 ID 列表：非空
    #[garde(length(min = 1))]
    pub tag_ids: Vec<i32>,
}

/// 覆盖式设置单张壁纸标签集合请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct SetWallpaperTagsRequest {
    /// 壁纸 ID：正整数
    #[garde(range(min = 1))]
    pub wallpaper_id: i32,
    /// 目标标签名全集：可空（清空标签），每个 trim 后 1~16 字符
    #[garde(inner(length(chars, min = 1, max = 16)))]
    pub tag_names: Vec<String>,
}

/// 查某壁纸标签请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct GetWallpaperTagsRequest {
    /// 壁纸 ID：正整数
    #[garde(range(min = 1))]
    pub wallpaper_id: i32,
}

/// 重命名标签请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct RenameTagRequest {
    /// 标签 ID：正整数
    #[garde(range(min = 1))]
    pub id: i32,
    /// 新标签名：trim 后 1~16 字符
    #[garde(length(chars, min = 1, max = 16))]
    pub name: String,
}

/// 删除标签请求
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct DeleteTagRequest {
    /// 标签 ID：正整数
    #[garde(range(min = 1))]
    pub id: i32,
}
