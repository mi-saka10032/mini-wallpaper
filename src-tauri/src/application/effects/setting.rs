//! 设置副作用
//!
//! 根据 setting key 执行对应的副作用逻辑，
//! 确保设置变更写入 DB 后立即生效。
//!
//! 所有方法通过 `impl TaskRegistry` 跨文件实现，
//! 天然拥有 `self.app`、`self.tasks` 等内部状态，零参数透传。

use log::{info, warn};

use super::AppEffects;
use crate::dto::app_setting_dto::keys as setting_keys;
use crate::services::{collection_service, monitor_config_service};
use crate::tasks::carousel::{carousel_key, CarouselTask};
use crate::tasks::fullscreen_detector::{FullscreenDetectionTask, FULLSCREEN_TIMER_KEY};

impl AppEffects<'_> {
    /// 根据 setting key 执行对应的副作用
    ///
    /// 每个需要"写入后立即生效"的 key 在此注册处理逻辑。
    /// 无副作用的 key（如 theme、language）仅写入 DB，前端自行响应。
    pub async fn apply_setting_side_effect(
        &mut self,
        key: &str,
        value: &str,
        monitor_id: Option<&str>,
    ) {
        match key {
            setting_keys::PAUSE_ON_FULLSCREEN => {
                // 开关只决定检测任务是否存在；is_running 判空避免重复设置时起多个轮询。
                let enabled = value == "true";
                if enabled {
                    if !self.is_running(FULLSCREEN_TIMER_KEY) {
                        self.spawn(FULLSCREEN_TIMER_KEY.to_string(), FullscreenDetectionTask);
                        info!("[TaskRegistry] 全屏检测已启动");
                    }
                } else {
                    self.stop(FULLSCREEN_TIMER_KEY);
                    info!("[TaskRegistry] 全屏检测已停止");
                }
            }
            setting_keys::GLOBAL_VOLUME => {
                // 音量是全局字段，Manager 的 reconcile 会 diff 出 VolumeChanged，
                // 一次派发同时更新 Native renderer 与广播到所有 WebView 窗口。
                let ctx = self.app_context();
                let mut guard = ctx.unified_manager.lock().await;
                if let Err(e) = guard
                    .reconcile(crate::context::unified_manager::ReconcileScope::All)
                    .await
                {
                    warn!("[TaskRegistry] 音量 reconcile 失败: {}", e);
                } else {
                    info!("[TaskRegistry] 音量已更新: {}", value);
                }
            }
            setting_keys::DISPLAY_MODE => {
                self.apply_display_mode_side_effect(value, monitor_id).await;
            }
            setting_keys::SHORTCUT_NEXT_WALLPAPER
            | setting_keys::SHORTCUT_PREV_WALLPAPER
            | setting_keys::SHORTCUT_TOGGLE_PAUSE
            | setting_keys::SHORTCUT_OPEN_MAIN
            | setting_keys::SHORTCUT_TOGGLE_FAVORITE => {
                // 键位已写入 DB，整组重注册使新键位即时生效、旧键位失效
                crate::platform::global_shortcut::reregister_all(&self.app).await;
                info!("[TaskRegistry] 快捷键 {} 变更，已触发重注册", key);
            }
            _ => {}
        }
    }

    /// display_mode 变更副作用
    ///
    /// 执行顺序：
    /// 1. 以 monitor_id 为基准，同步配置到所有从属 monitor（mirror/extend 模式）
    /// 1.5 Native 侧整组重下发壁纸 / fitMode：renderer 不会像 WebView 页面那样
    ///     收到 display-mode-changed 后重读 DB，DB 同步后必须显式推一次
    /// 2. 通知所有壁纸窗口切换渲染模式（wallpaper_manager 广播 display-mode-changed）
    /// 3. 管理定时器：主 monitor 保持/启动定时器，从属 monitor 停止定时器
    async fn apply_display_mode_side_effect(
        &mut self,
        display_mode: &str,
        monitor_id: Option<&str>,
    ) {
        info!("[TaskRegistry] display_mode 变更: {}", display_mode);

        // 同步组只启一个 timer、独立模式每屏一个，牌堆 key 的映射整体改变。
        // 清空后下一次消费会按新的 carousel_key 以当前显示图为锚重建。
        self.ctx.play_queues().clear();

        let db = self.db();

        // 架构重整 Phase 3：display_mode 的渲染副作用（set_display_mode、配置同步、
        // 整组重下发、WebView 广播）全部由 UnifiedWallpaperManager 的一次 reconcile 承担。
        // 这里只保留无法从「窗口状态 diff」推导的部分：timer 的启停策略。
        let configs = match monitor_config_service::get_all(&db).await {
            Ok(c) => c,
            Err(e) => {
                warn!("[TaskRegistry] 获取 monitor configs 失败: {}", e);
                return;
            }
        };

        let active_configs: Vec<_> = configs.iter().filter(|c| c.active).collect();
        if active_configs.is_empty() {
            return;
        }

        // mirror/extend：以基准屏配置单事务同步到所有从属屏，再触发一次 reconcile。
        // independent：DB 无批量变更，直接触发一次 reconcile 让所有屏按新 is_sync 重下发。
        if monitor_config_service::is_sync_mode(display_mode) {
            // 默认基准屏用与 start_all_carousel_timers 相同的确定性口径；
            // 用户显式指定时仍优先（决定用谁的配置覆盖整组）。
            let default_primary = monitor_config_service::sync_group_primary(&db)
                .await
                .ok()
                .flatten()
                .unwrap_or_default();
            let primary_mid = monitor_id.unwrap_or(default_primary.as_str());
            if let Some(source) = active_configs.iter().find(|c| c.monitor_id == primary_mid) {
                let targets: Vec<String> = active_configs
                    .iter()
                    .filter(|c| c.monitor_id != primary_mid)
                    .map(|c| c.monitor_id.clone())
                    .collect();
                if let Err(e) =
                    monitor_config_service::update_monitors_batch(&db, source, &targets).await
                {
                    warn!("[TaskRegistry] 批量同步配置失败: {}", e);
                    return;
                }
            }
        }

        {
            let ctx = self.app_context();
            let mut guard = ctx.unified_manager.lock().await;
            if let Err(e) = guard
                .reconcile(crate::context::unified_manager::ReconcileScope::All)
                .await
            {
                warn!("[TaskRegistry] display_mode reconcile 失败: {}", e);
            }
        }

        match display_mode {
            "independent" => {
                // 恢复所有满足条件的定时器
                for config in &active_configs {
                    let key = carousel_key(&config.monitor_id);
                    if monitor_config_service::should_start_timer(config) {
                        if let Some(cid) = config.collection_id {
                            if let Ok(true) =
                                collection_service::has_enough_wallpapers(&db, cid).await
                            {
                                if !self.is_running(&key) {
                                    self.spawn(
                                        key,
                                        CarouselTask {
                                            monitor_id: config.monitor_id.clone(),
                                        },
                                    );
                                    info!("[TaskRegistry] 恢复定时器: {}", config.monitor_id);
                                }
                            }
                        }
                    }
                }
            }
            "mirror" | "extend" => {
                let default_primary = monitor_config_service::sync_group_primary(&db)
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                let primary_mid = monitor_id.unwrap_or(default_primary.as_str());
                let primary_config = active_configs.iter().find(|c| c.monitor_id == primary_mid);

                // 定时器管理（Phase 3 范围排除）：仅保留主 monitor 的定时器
                for config in &active_configs {
                    if config.monitor_id != primary_mid {
                        let key = carousel_key(&config.monitor_id);
                        if self.is_running(&key) {
                            self.stop(&key);
                            info!("[TaskRegistry] 停止从属定时器: {}", config.monitor_id);
                        }
                    }
                }

                // 检查主 monitor 是否具备启动定时器条件
                if let Some(source) = primary_config {
                    let key = carousel_key(&source.monitor_id);
                    if monitor_config_service::should_start_timer(source) {
                        if let Some(cid) = source.collection_id {
                            match collection_service::has_enough_wallpapers(&db, cid).await {
                                Ok(true) => {
                                    self.restart(
                                        key,
                                        CarouselTask {
                                            monitor_id: source.monitor_id.clone(),
                                        },
                                    );
                                    info!("[TaskRegistry] 主定时器已启动: {}", source.monitor_id);
                                }
                                Ok(false) => {
                                    self.stop(&key);
                                }
                                Err(e) => {
                                    warn!("[TaskRegistry] 检查收藏夹失败: {}", e);
                                    self.stop(&key);
                                }
                            }
                        } else {
                            self.stop(&key);
                        }
                    } else {
                        self.stop(&key);
                    }
                }
            }
            _ => {}
        }
    }
}
