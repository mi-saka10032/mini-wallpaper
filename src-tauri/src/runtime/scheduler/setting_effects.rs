//! 设置副作用
//!
//! 根据 setting key 执行对应的副作用逻辑，
//! 确保设置变更写入 DB 后立即生效。
//!
//! 所有方法通过 `impl Scheduler` 跨文件实现，
//! 天然拥有 `self.app`、`self.tasks` 等内部状态，零参数透传。

use log::{info, warn};

use super::Scheduler;
use crate::dto::app_setting_dto::keys as setting_keys;
use crate::events::VolumeChangedPayload;
use crate::runtime::tasks::carousel::{carousel_key, CarouselTask};
use crate::runtime::tasks::fullscreen_detector::{FullscreenDetectionTask, FULLSCREEN_TIMER_KEY};
use crate::services::{collection_service, monitor_config_service};

impl Scheduler {
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
                let enabled = value == "true";
                if enabled {
                    if !self.is_running(FULLSCREEN_TIMER_KEY) {
                        self.spawn(
                            FULLSCREEN_TIMER_KEY.to_string(),
                            FullscreenDetectionTask,
                        );
                        info!("[Scheduler] 全屏检测已启动");
                    }
                } else {
                    self.stop(FULLSCREEN_TIMER_KEY);
                    info!("[Scheduler] 全屏检测已停止");
                }
            }
            setting_keys::GLOBAL_VOLUME => {
                let volume = value.parse::<u32>().unwrap_or(0).min(100);
                let wm = self.window_manager();
                let mgr = wm.lock().await;
                mgr.broadcast(&VolumeChangedPayload { volume: volume as f64 });
                info!("[Scheduler] 音量已更新: {}%", volume);
            }
            setting_keys::DISPLAY_MODE => {
                self.apply_display_mode_side_effect(value, monitor_id).await;
            }
            _ => {}
        }
    }

    /// display_mode 变更副作用
    ///
    /// 执行顺序：
    /// 1. 以 monitor_id 为基准，同步配置到所有从属 monitor（mirror/extend 模式）
    /// 2. 通知所有壁纸窗口切换渲染模式（wallpaper_manager 广播 display-mode-changed）
    /// 3. 管理定时器：主 monitor 保持/启动定时器，从属 monitor 停止定时器
    async fn apply_display_mode_side_effect(
        &mut self,
        display_mode: &str,
        monitor_id: Option<&str>,
    ) {
        info!("[Scheduler] display_mode 变更: {}", display_mode);

        let db = self.db();
        let wm = self.window_manager();

        let configs = match monitor_config_service::get_all(&db).await {
            Ok(c) => c,
            Err(e) => {
                warn!("[Scheduler] 获取 monitor configs 失败: {}", e);
                return;
            }
        };

        let active_configs: Vec<_> = configs.iter().filter(|c| c.active).collect();
        if active_configs.is_empty() {
            return;
        }

        match display_mode {
            "independent" => {
                // 通知所有壁纸窗口切换到 independent
                {
                    let mgr = wm.lock().await;
                    for config in &active_configs {
                        if let Err(e) =
                            mgr.notify_display_mode_changed(&config.monitor_id, "independent")
                        {
                            warn!(
                                "[Scheduler] 发送 display-mode-changed 失败 {}: {}",
                                config.monitor_id, e
                            );
                        }
                    }
                }

                // 恢复所有满足条件的定时器
                for config in &active_configs {
                    let key = carousel_key(&config.monitor_id);
                    if monitor_config_service::should_start_timer(config) {
                        if let Some(cid) = config.collection_id {
                            match collection_service::has_enough_wallpapers(&db, cid).await {
                                Ok(true) => {
                                    if !self.is_running(&key) {
                                        self.spawn(
                                            key,
                                            CarouselTask {
                                                monitor_id: config.monitor_id.clone(),
                                            },
                                        );
                                        info!(
                                            "[Scheduler] 恢复定时器: {}",
                                            config.monitor_id
                                        );
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            "mirror" | "extend" => {
                // 步骤 1: 以 monitor_id 为基准，同步配置到所有从属 monitor
                let primary_mid = monitor_id.unwrap_or_else(|| {
                    active_configs
                        .first()
                        .map(|c| c.monitor_id.as_str())
                        .unwrap_or("")
                });

                let primary_config =
                    active_configs.iter().find(|c| c.monitor_id == primary_mid);
                if let Some(source) = primary_config {
                    for config in &active_configs {
                        if config.monitor_id == primary_mid {
                            continue;
                        }
                        if let Err(e) = monitor_config_service::sync_config_from(
                            &db,
                            &config.monitor_id,
                            source,
                        )
                        .await
                        {
                            warn!(
                                "[Scheduler] 同步配置到 {} 失败: {}",
                                config.monitor_id, e
                            );
                        }
                    }
                }

                // 步骤 2: 通知所有壁纸窗口切换渲染模式
                {
                    let mgr = wm.lock().await;
                    for config in &active_configs {
                        if let Err(e) =
                            mgr.notify_display_mode_changed(&config.monitor_id, display_mode)
                        {
                            warn!(
                                "[Scheduler] 发送 display-mode-changed 失败 {}: {}",
                                config.monitor_id, e
                            );
                        }
                    }
                }

                // 步骤 3: 定时器管理 — 仅保留主 monitor 的定时器，停止所有从属定时器
                for config in &active_configs {
                    if config.monitor_id != primary_mid {
                        let key = carousel_key(&config.monitor_id);
                        if self.is_running(&key) {
                            self.stop(&key);
                            info!(
                                "[Scheduler] 停止从属定时器: {}",
                                config.monitor_id
                            );
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
                                    info!(
                                        "[Scheduler] 主定时器已启动: {}",
                                        source.monitor_id
                                    );
                                }
                                Ok(false) => {
                                    self.stop(&key);
                                }
                                Err(e) => {
                                    warn!("[Scheduler] 检查收藏夹失败: {}", e);
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
