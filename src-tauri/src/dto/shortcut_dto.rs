use serde::Deserialize;

/// 切换壁纸方向
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Next,
    Prev,
}
