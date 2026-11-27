//! 场景快照的数据契约。
//!
//! `SceneState` 同时是「存进 payload_json 的内容」和「弹窗里展示的当前状态」，
//! 保证用户看到什么就保存什么。展示用的壁纸名 / 收藏夹名由前端按 id 从已有
//! store 查，不写进快照——改名后仍显示新名字，不会被旧快照固化。

use garde::Validate;
use serde::{Deserialize, Serialize};

/// 单屏桌面状态。只捕业务字段：`active` 是物理状态、`id`/`updated_at` 是存储细节。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneMonitorState {
    pub monitor_id: String,
    pub wallpaper_id: Option<i32>,
    pub collection_id: Option<i32>,
    pub fit_mode: String,
    pub play_mode: String,
    pub play_interval: i32,
    pub is_enabled: bool,
}

/// 完整场景状态：两个全局设置 + 逐屏配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneState {
    pub display_mode: String,
    pub global_volume: String,
    pub monitors: Vec<SceneMonitorState>,
}

/// 列表项：附带解析后的状态，前端可直接展示已存场景的内容。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneSummary {
    pub id: i32,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub state: SceneState,
}

/// 应用结果：`skipped` 汇总被跳过的屏与原因，供前端 Toast 提示。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplySceneResult {
    pub applied: usize,
    pub skipped: Vec<String>,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct SaveSceneRequest {
    #[garde(length(chars, min = 1, max = 32))]
    pub name: String,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct RenameSceneRequest {
    #[garde(range(min = 1))]
    pub id: i32,
    #[garde(length(chars, min = 1, max = 32))]
    pub name: String,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct SceneIdRequest {
    #[garde(range(min = 1))]
    pub id: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_state_round_trip_keeps_camel_case_and_null_refs() {
        let state = SceneState {
            display_mode: "mirror".to_string(),
            global_volume: "40".to_string(),
            monitors: vec![SceneMonitorState {
                monitor_id: r"\\.\DISPLAY1".to_string(),
                wallpaper_id: None,
                collection_id: Some(7),
                fit_mode: "cover".to_string(),
                play_mode: "random".to_string(),
                play_interval: 600,
                is_enabled: true,
            }],
        };

        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"displayMode\":\"mirror\""));
        assert!(json.contains("\"globalVolume\":\"40\""));
        assert!(json.contains("\"wallpaperId\":null"));
        assert!(json.contains("\"collectionId\":7"));
        assert!(!json.contains("display_mode"));

        assert_eq!(serde_json::from_str::<SceneState>(&json).unwrap(), state);
    }
}
