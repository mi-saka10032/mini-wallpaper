//! 播放次数副作用。
//!
//! `play_count` 表达「被设为当前壁纸的次数」。所有会更新
//! `monitor_configs.wallpaper_id` 的用例在写库成功后调用本钩子：
//! 轮播 tick、快捷键 / 托盘切换、删除联动换图、手动选壁纸、场景恢复。
//!
//! 同步组由调用方保证一次切换只调用一次（组写入本身就是一次播放）；
//! 计数失败只告警，绝不阻断切换主流程——统计数据不该影响桌面显示。

use log::warn;

use super::AppEffects;
use crate::services::wallpaper_service;

impl AppEffects<'_> {
    /// 记录一次播放；`None` 表示整组清空显示，不计。
    pub async fn record_wallpaper_play(&self, wallpaper_id: Option<i32>) {
        let Some(id) = wallpaper_id else {
            return;
        };
        if let Err(e) = wallpaper_service::increment_play_count(&self.ctx.db, id).await {
            warn!("[PlayCount] 记录播放失败 id={}: {:#}", id, e);
        }
    }
}
