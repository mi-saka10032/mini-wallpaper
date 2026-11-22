//! 渲染协调器
//!
//! 壁纸渲染链路的唯一入口：集中管理每个显示器的 active backend、
//! 决定 Native / WebView 路由，并把「换图 / 清除 / fitMode / 全屏」等
//! 通知翻译成对应 backend 的操作。
//!
//! ## 为什么不放在 WallpaperWindowManager
//!
//! WM 只负责 WebView 窗口的创建与事件发送，不持有 db / renderer，无法判断
//! 壁纸能否被原生解码。把 backend 决策塞进 WM 会让它同时依赖 db 与 renderer，
//! 且通知方法是同步 emit，无法承载「查库 → 决策 → 路由」的异步链。
//! 独立协调器保持 WM 纯粹，调用方只锁本协调器，避免与 WM 的双锁持锁。
//!
//! ## backend 选择策略
//!
//! - Native 可支持的格式（图片 / GIF / WebP / mp4 / m4v）一律走原生渲染
//! - 仅当原生不支持时才回退 WebView（Auto 模式；NativeOnly 下直接报错）
//! - backend 在 `ensure_target` / `update_wallpaper` 时按当前壁纸重新决策

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use sea_orm::DatabaseConnection;
use tauri::AppHandle;
use tokio::sync::Mutex;

use super::window_manager::WallpaperWindowManager;
use crate::entities::wallpaper;
use crate::events::{RendererErrorPayload, ThumbnailChangedPayload, TypedEmit};
use crate::platform::wallpaper_renderer::{
    default_backend_selection, resolve_backend, BackendSelection, DesktopRenderer, FitMode,
    MonitorRect, RenderBackend, RendererError, RendererErrorCode, WallpaperKind,
};
use crate::services::{monitor_config_service, wallpaper_service};

/// 批量通知指令，供数据库阶段收集、短锁内一次性派发
pub enum RenderNotification {
    Updated(String, i32, bool),
    Cleared(String, bool),
}

/// 渲染协调器：壁纸渲染链路的唯一入口
pub struct RenderCoordinator {
    db: DatabaseConnection,
    app_handle: AppHandle,
    window_manager: Arc<Mutex<WallpaperWindowManager>>,
    renderer: DesktopRenderer,
    selection: BackendSelection,
    /// monitor_id -> 当前 active backend
    backends: HashMap<String, RenderBackend>,
    /// monitor_id -> 显示器几何，供 backend 切换时重建 surface / 窗口
    rects: HashMap<String, MonitorRect>,
}

impl RenderCoordinator {
    pub fn new(
        db: DatabaseConnection,
        app_handle: AppHandle,
        window_manager: Arc<Mutex<WallpaperWindowManager>>,
    ) -> Result<Self> {
        Ok(Self {
            db,
            app_handle,
            window_manager,
            renderer: DesktopRenderer::start()?,
            selection: default_backend_selection(),
            backends: HashMap::new(),
            rects: HashMap::new(),
        })
    }

    // ==================== 公共入口 ====================

    /// 为显示器准备渲染目标（首帧）。
    ///
    /// 按当前壁纸解析 backend：Native 则建原生 surface 并渲染首帧，
    /// WebView 则沿用现有壁纸窗口流程。后端不支持的格式直接报错并上报事件。
    pub async fn ensure_target(
        &mut self,
        monitor_id: &str,
        rect: MonitorRect,
        wallpaper_id: i32,
    ) -> Result<RenderBackend> {
        let descriptor = match self.build_descriptor(monitor_id, wallpaper_id).await {
            Ok(d) => d,
            Err(e) => {
                self.report_error(Some(monitor_id), Some(wallpaper_id), &e);
                return Err(e.into());
            }
        };

        match descriptor.backend {
            RenderBackend::Native => {
                self.renderer.ensure_monitor(monitor_id.to_string(), rect)?;
                self.renderer.set_wallpaper(
                    monitor_id.to_string(),
                    descriptor.file_path,
                    descriptor.kind,
                    descriptor.fit_mode,
                )?;
            }
            RenderBackend::WebView => {
                let mut wm = self.window_manager.lock().await;
                wm.create_and_show(
                    monitor_id,
                    rect.x,
                    rect.y,
                    rect.width as u32,
                    rect.height as u32,
                )
                .await
                .map_err(anyhow::Error::msg)?;
            }
        }

        self.rects.insert(monitor_id.to_string(), rect);
        self.backends
            .insert(monitor_id.to_string(), descriptor.backend);
        Ok(descriptor.backend)
    }

    /// 销毁显示器的渲染目标，按当前 backend 选择清理路径
    pub async fn destroy_target(&mut self, monitor_id: &str) {
        let backend = self.backends.remove(monitor_id);
        self.rects.remove(monitor_id);
        match backend {
            Some(RenderBackend::Native) => {
                if let Err(e) = self.renderer.destroy_monitor(monitor_id.to_string()) {
                    log::warn!("[RenderCoordinator] 销毁原生表面失败 {}: {}", monitor_id, e);
                }
            }
            Some(RenderBackend::WebView) | None => {
                let mut wm = self.window_manager.lock().await;
                wm.destroy_window(monitor_id);
            }
        }
    }

    /// 壁纸变更：按新壁纸重新决策 backend，需要时切换渲染链路
    pub async fn update_wallpaper(&mut self, monitor_id: &str, wallpaper_id: i32, is_sync: bool) {
        let targets = self.resolve_targets(monitor_id, is_sync);
        for mid in targets {
            let descriptor = match self.build_descriptor(&mid, wallpaper_id).await {
                Ok(d) => d,
                Err(e) => {
                    self.report_error(Some(&mid), Some(wallpaper_id), &e);
                    continue;
                }
            };

            if let Err(e) = self.apply_backend(&mid, wallpaper_id, &descriptor).await {
                log::warn!("[RenderCoordinator] 壁纸更新失败 {}: {:#}", mid, e);
                self.report_error(
                    Some(&mid),
                    Some(wallpaper_id),
                    &RendererError::new(RendererErrorCode::BackendUnavailable, e.to_string()),
                );
                continue;
            }

            // 主窗口缩略图与 backend 无关，始终通知刷新
            if let Err(e) = self.app_handle.typed_emit(&ThumbnailChangedPayload {
                monitor_id: mid.clone(),
                wallpaper_id,
            }) {
                log::warn!(
                    "[RenderCoordinator] thumbnail-changed 发送失败 {}: {}",
                    mid,
                    e
                );
            }
        }
    }

    /// 清空壁纸：Native 走 renderer，WebView 走现有事件
    pub async fn clear_wallpaper(&mut self, monitor_id: &str, is_sync: bool) {
        let targets = self.resolve_targets(monitor_id, is_sync);
        for mid in targets {
            let backend = self
                .backends
                .get(&mid)
                .copied()
                .unwrap_or(RenderBackend::WebView);
            match backend {
                RenderBackend::Native => {
                    if let Err(e) = self.renderer.clear_wallpaper(mid.clone()) {
                        log::warn!("[RenderCoordinator] 清空原生壁纸失败 {}: {}", mid, e);
                    }
                }
                RenderBackend::WebView => {
                    let wm = self.window_manager.lock().await;
                    wm.notify_wallpaper_cleared(&mid, false);
                }
            }
        }
    }

    /// fitMode 变更：Native 走 renderer，WebView 走现有事件
    pub async fn set_fit_mode(&self, monitor_id: &str, fit_mode: Option<&str>, is_sync: bool) {
        let Some(fit_mode) = fit_mode.and_then(FitMode::parse) else {
            return;
        };
        let targets = self.resolve_targets(monitor_id, is_sync);
        for mid in targets {
            let backend = self
                .backends
                .get(&mid)
                .copied()
                .unwrap_or(RenderBackend::WebView);
            match backend {
                RenderBackend::Native => {
                    if let Err(e) = self.renderer.set_fit_mode(mid.clone(), fit_mode) {
                        log::warn!("[RenderCoordinator] 原生 fitMode 更新失败 {}: {}", mid, e);
                    }
                }
                RenderBackend::WebView => {
                    let wm = self.window_manager.lock().await;
                    if let Err(e) = wm.notify_fit_mode_changed(&mid, fit_mode.as_str()) {
                        log::warn!(
                            "[RenderCoordinator] WebView fitMode 更新失败 {}: {}",
                            mid,
                            e
                        );
                    }
                }
            }
        }
    }

    /// 批量派发已收集的通知，锁的持有范围由调用方收窄
    pub async fn dispatch_notifications(&mut self, notifications: Vec<RenderNotification>) {
        for notification in notifications {
            match notification {
                RenderNotification::Updated(mid, wid, is_sync) => {
                    self.update_wallpaper(&mid, wid, is_sync).await;
                }
                RenderNotification::Cleared(mid, is_sync) => {
                    self.clear_wallpaper(&mid, is_sync).await;
                }
            }
        }
    }

    /// 全屏暂停 / 恢复：两种 backend 都暂停（WebView 隐藏窗口，Native 停帧）
    pub async fn set_paused(&self, paused: bool) {
        if let Err(e) = self.renderer.set_paused(paused) {
            log::warn!("[RenderCoordinator] 原生渲染暂停切换失败: {}", e);
        }
        let wm = self.window_manager.lock().await;
        if paused {
            wm.hide_all();
        } else {
            wm.show_all();
        }
    }

    /// 当前已创建的渲染目标 monitor_id 列表（两种 backend 合并）
    pub async fn active_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.backends.keys().cloned().collect();
        let wm = self.window_manager.lock().await;
        for id in wm.get_active_window_ids() {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids
    }

    // ==================== 内部辅助 ====================

    /// 壁纸变更的目标集合：同步模式取所有已建目标，独立模式仅指定显示器
    fn resolve_targets(&self, monitor_id: &str, is_sync: bool) -> Vec<String> {
        if !is_sync {
            return vec![monitor_id.to_string()];
        }
        self.backends.keys().cloned().collect()
    }

    /// 组装渲染描述：查壁纸 + config，解析 backend 与 fit_mode
    async fn build_descriptor(
        &self,
        monitor_id: &str,
        wallpaper_id: i32,
    ) -> Result<Descriptor, RendererError> {
        let wallpaper = wallpaper_service::get_active_by_id(&self.db, wallpaper_id)
            .await
            .map_err(|e| {
                RendererError::new(
                    RendererErrorCode::DecodeFailed,
                    format!("查询壁纸 {} 失败: {}", wallpaper_id, e),
                )
            })?
            .ok_or_else(|| {
                RendererError::new(
                    RendererErrorCode::DecodeFailed,
                    format!("壁纸 {} 不存在或已在回收站", wallpaper_id),
                )
            })?;

        let kind = kind_from_model(&wallpaper)?;
        let backend = resolve_backend(
            kind,
            PathBuf::from(&wallpaper.file_path).as_path(),
            self.selection,
        )?;
        let fit_mode = self.resolve_fit_mode(monitor_id).await;

        Ok(Descriptor {
            file_path: PathBuf::from(&wallpaper.file_path),
            kind,
            backend,
            fit_mode,
        })
    }

    async fn resolve_fit_mode(&self, monitor_id: &str) -> FitMode {
        match monitor_config_service::get_by_monitor_id(&self.db, monitor_id).await {
            Ok(Some(config)) => FitMode::parse(&config.fit_mode).unwrap_or(FitMode::Cover),
            _ => FitMode::Cover,
        }
    }

    /// 按 backend 应用壁纸；跨 backend 切换时先销毁旧目标再建新目标
    async fn apply_backend(
        &mut self,
        monitor_id: &str,
        wallpaper_id: i32,
        descriptor: &Descriptor,
    ) -> Result<()> {
        let previous = self.backends.get(monitor_id).copied();

        match descriptor.backend {
            RenderBackend::Native => {
                if previous != Some(RenderBackend::Native) {
                    // 从 WebView 切到 Native：先销毁 WebView 窗口，再建原生 surface
                    {
                        let mut wm = self.window_manager.lock().await;
                        wm.destroy_window(monitor_id);
                    }
                    let rect = self.rects.get(monitor_id).copied().ok_or_else(|| {
                        RendererError::new(
                            RendererErrorCode::BackendUnavailable,
                            format!("monitor {} 缺少几何信息，无法切换原生渲染", monitor_id),
                        )
                    })?;
                    self.renderer.ensure_monitor(monitor_id.to_string(), rect)?;
                }
                self.renderer.set_wallpaper(
                    monitor_id.to_string(),
                    descriptor.file_path.clone(),
                    descriptor.kind,
                    descriptor.fit_mode,
                )?;
            }
            RenderBackend::WebView => {
                if previous != Some(RenderBackend::WebView) {
                    // 从 Native 切回 WebView：销毁原生 surface，重建壁纸窗口
                    self.renderer.destroy_monitor(monitor_id.to_string())?;
                    let rect = self.rects.get(monitor_id).copied().ok_or_else(|| {
                        RendererError::new(
                            RendererErrorCode::BackendUnavailable,
                            format!("monitor {} 缺少几何信息，无法切换 WebView 渲染", monitor_id),
                        )
                    })?;
                    let mut wm = self.window_manager.lock().await;
                    wm.create_and_show(
                        monitor_id,
                        rect.x,
                        rect.y,
                        rect.width as u32,
                        rect.height as u32,
                    )
                    .await
                    .map_err(anyhow::Error::msg)?;
                    // 新建窗口后需要发一次 wallpaper-changed 让前端加载壁纸
                    wm.update_window(monitor_id, wallpaper_id)
                        .map_err(anyhow::Error::msg)?;
                } else {
                    let wm = self.window_manager.lock().await;
                    wm.update_window(monitor_id, wallpaper_id)
                        .map_err(anyhow::Error::msg)?;
                }
            }
        }

        self.backends
            .insert(monitor_id.to_string(), descriptor.backend);
        Ok(())
    }

    fn report_error(
        &self,
        monitor_id: Option<&str>,
        wallpaper_id: Option<i32>,
        error: &RendererError,
    ) {
        let payload = RendererErrorPayload {
            monitor_id: monitor_id.map(str::to_string),
            wallpaper_id,
            code: error.code.as_str().to_string(),
            message: error.message.clone(),
        };
        if let Err(e) = self.app_handle.typed_emit(&payload) {
            log::warn!("[RenderCoordinator] renderer-error 事件发送失败: {}", e);
        }
    }
}

/// 内部渲染描述（只含渲染所需字段，不涉及 DB 实体）
struct Descriptor {
    file_path: PathBuf,
    kind: WallpaperKind,
    backend: RenderBackend,
    fit_mode: FitMode,
}

/// 从壁纸实体推断类型；type 字段由上传时的扩展名检测写入
fn kind_from_model(wallpaper: &wallpaper::Model) -> Result<WallpaperKind, RendererError> {
    match wallpaper.r#type.as_str() {
        "image" => Ok(WallpaperKind::Image),
        "gif" => Ok(WallpaperKind::Gif),
        "video" => Ok(WallpaperKind::Video),
        other => Err(RendererError::new(
            RendererErrorCode::UnsupportedFormat,
            format!("未知壁纸类型: {}", other),
        )),
    }
}
