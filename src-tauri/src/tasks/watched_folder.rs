//! 目录监听后台任务：存活探测 + 非 active 夹的完整 diff 同步。
//!
//! 周期可在 app_settings 里配置，每轮重新读取，改设置无需重启任务。
//! 存活探测**始终**运行（不受 active 暂停影响），否则发现不了「有效→失效」。

use std::time::Duration;

use tokio::task::JoinHandle;

use crate::services::watched_folder_service;
use crate::tasks::{TaskDeps, TaskSpawner};

pub struct WatchedFolderTask;

impl TaskSpawner for WatchedFolderTask {
    fn spawn(self, deps: TaskDeps) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                let secs = watched_folder_service::scan_interval_seconds(&deps.ctx.db).await;
                tokio::time::sleep(Duration::from_secs(secs)).await;
                if let Err(e) = watched_folder_service::probe_and_sync(&deps.ctx).await {
                    log::warn!("[Watched] 后台探测失败: {e:#}");
                }
            }
        })
    }
}
