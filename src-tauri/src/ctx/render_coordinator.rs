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
//!
//! ## M5：跨屏组编排
//!
//! `rects` 覆盖所有 active 显示器（即使该屏暂时没有壁纸），其并集即 extend 的
//! 虚拟桌面画布。同步模式下的换图只查库一次、只解码一次，整组 surface 共享同一个
//! `Arc<SharedFrames>`，因此帧时钟天然一致。任一屏几何 / backend / display_mode
//! 变化都会触发 `refresh_group_geometry()` 重算全组视口——只更新单屏会让接缝错位。
//!
//! D2：extend 组内 backend 不全是 Native 时降级为 M4 的逐屏独立渲染并提示一次，
//! 避免 Rust 侧几何与 WebView 侧 `useExtendViewport` 两套坐标在接缝处对不上。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use sea_orm::DatabaseConnection;
use tauri::AppHandle;
use tokio::sync::Mutex;

use super::window_manager::WallpaperWindowManager;
use crate::dto::app_setting_dto::keys as setting_keys;
use crate::entities::wallpaper;
use crate::events::{RendererErrorPayload, ThumbnailChangedPayload, TypedEmit};
use crate::platform::wallpaper_renderer::{
    default_backend_selection, group_viewports, resolve_backend, BackendSelection, DesktopRenderer,
    DisplayMode, ExtendViewport, FitMode, MonitorRect, RenderBackend, RendererError,
    RendererErrorCode, WallpaperKind,
};
use crate::services::{app_setting_service, monitor_config_service, wallpaper_service};

/// 批量通知指令，供数据库阶段收集、短锁内一次性派发
pub enum RenderNotification {
    Updated(String, i32, bool),
    Cleared(String, bool),
}

/// extend 组降级时使用的 code；前端据此改用 warning 级 toast。
const CODE_EXTEND_DEGRADED: &str = "ExtendDegraded";

/// 渲染协调器：壁纸渲染链路的唯一入口
pub struct RenderCoordinator {
    db: DatabaseConnection,
    app_handle: AppHandle,
    window_manager: Arc<Mutex<WallpaperWindowManager>>,
    renderer: DesktopRenderer,
    selection: BackendSelection,
    /// monitor_id -> 当前 active backend
    backends: HashMap<String, RenderBackend>,
    /// monitor_id -> 当前正在显示的壁纸 id；用于判定换图是否触发 crossfade。
    /// 与 renderer 侧的 transition 队列解耦：这里只决定要不要 fade，不跟踪 fade 进度。
    wallpaper_ids: HashMap<String, i32>,
    /// monitor_id -> 显示器几何；extend 的 union 由整张表算出
    rects: HashMap<String, MonitorRect>,
    display_mode: DisplayMode,
    /// 降级提示只发一次，恢复为全 Native 时重置
    degraded_logged: bool,
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
            wallpaper_ids: HashMap::new(),
            rects: HashMap::new(),
            display_mode: DisplayMode::Independent,
            degraded_logged: false,
        })
    }

    // ==================== 公共入口 ====================

    /// 从 app_setting 读取 display_mode；启动时与设置变更后各调用一次。
    pub async fn refresh_display_mode(&mut self) {
        let mode = match app_setting_service::get(&self.db, setting_keys::DISPLAY_MODE).await {
            Ok(Some(value)) => DisplayMode::parse(&value).unwrap_or(DisplayMode::Independent),
            _ => DisplayMode::Independent,
        };
        self.set_display_mode(mode);
    }

    /// 从 app_setting 读取 global_volume；启动时同步一次，避免持久化音量在重启后失效。
    pub async fn refresh_volume(&mut self) {
        let volume = match app_setting_service::get(&self.db, setting_keys::GLOBAL_VOLUME).await {
            Ok(Some(value)) => value.parse::<u32>().unwrap_or(0).min(100),
            _ => 0,
        };
        self.set_volume(volume);
    }

    pub fn set_volume(&self, volume: u32) {
        if let Err(err) = self.renderer.set_volume(volume) {
            log::warn!("[RenderCoordinator] 同步 Native 音量失败: {:#}", err);
        }
    }

    pub fn set_display_mode(&mut self, mode: DisplayMode) {
        if self.display_mode == mode {
            return;
        }
        self.display_mode = mode;
        self.degraded_logged = false;
        self.refresh_group_geometry();
    }

    /// 登记显示器几何。必须在判断「有没有壁纸」之前调用，否则没有壁纸的屏
    /// 不会进入 union，extend 画布会缺一块。
    pub fn register_monitor(&mut self, monitor_id: &str, rect: MonitorRect) {
        if self.rects.get(monitor_id).copied() == Some(rect) {
            return;
        }
        self.rects.insert(monitor_id.to_string(), rect);
        self.refresh_group_geometry();
    }

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
        self.rects.insert(monitor_id.to_string(), rect);

        let descriptor = match self.build_descriptor(monitor_id, wallpaper_id).await {
            Ok(d) => d,
            Err(e) => {
                self.report_error(Some(monitor_id), Some(wallpaper_id), &e);
                return Err(e.into());
            }
        };

        match descriptor.backend {
            RenderBackend::Native => {
                if let Err(e) = self
                    .renderer
                    .ensure_monitor(monitor_id.to_string(), rect)
                    .await
                {
                    let error =
                        RendererError::new(RendererErrorCode::BackendUnavailable, e.to_string());
                    self.report_error(Some(monitor_id), Some(wallpaper_id), &error);
                    return Err(error.into());
                }
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
        self.backends
            .insert(monitor_id.to_string(), descriptor.backend);

        if descriptor.backend == RenderBackend::Native {
            // 同步模式下每建好一屏就重发整组：最后一次下发的 Arc 覆盖全组，
            // 保证所有屏共享同一帧时钟，而不是各自留存一份。
            let mids = if self.display_mode == DisplayMode::Independent {
                vec![monitor_id.to_string()]
            } else {
                self.native_targets()
            };
            if let Err(e) = self
                .renderer
                .set_wallpaper(
                    mids.clone(),
                    descriptor.file_path.clone(),
                    descriptor.kind,
                    descriptor.fit_mode,
                    false, // 首帧硬切：没有 from 可淡出
                )
                .await
            {
                let error = RendererError::new(RendererErrorCode::DecodeFailed, e.to_string());
                self.report_error(Some(monitor_id), Some(wallpaper_id), &error);
                return Err(error.into());
            }
            for mid in mids {
                self.wallpaper_ids.insert(mid, wallpaper_id);
            }
        }

        self.refresh_group_geometry();
        Ok(descriptor.backend)
    }

    /// 销毁显示器的渲染目标，按当前 backend 选择清理路径
    pub async fn destroy_target(&mut self, monitor_id: &str) {
        let backend = self.backends.remove(monitor_id);
        self.wallpaper_ids.remove(monitor_id);
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
        self.refresh_group_geometry();
    }

    /// 壁纸变更：整组只查库一次，Native 侧一次解码共享给所有屏
    pub async fn update_wallpaper(&mut self, monitor_id: &str, wallpaper_id: i32, is_sync: bool) {
        let targets = self.resolve_targets(monitor_id, is_sync);
        let Some(first) = targets.first().cloned() else {
            return;
        };
        let descriptor = match self.build_descriptor(&first, wallpaper_id).await {
            Ok(d) => d,
            Err(e) => {
                self.report_error(Some(&first), Some(wallpaper_id), &e);
                return;
            }
        };

        // apply_backend 会改写 backends；crossfade 必须基于切换「之前」的状态判断，
        // 否则 Native→WebView→Native 的回切会被误判成 Native→Native。
        let previous: HashMap<String, (Option<RenderBackend>, Option<i32>)> = targets
            .iter()
            .map(|mid| {
                (
                    mid.clone(),
                    (
                        self.backends.get(mid).copied(),
                        self.wallpaper_ids.get(mid).copied(),
                    ),
                )
            })
            .collect();

        // 阶段 1：只切换 backend / 创建或销毁渲染目标，不设置内容
        for mid in &targets {
            if let Err(e) = self.apply_backend(mid, &descriptor).await {
                log::warn!("[RenderCoordinator] 壁纸更新失败 {}: {:#}", mid, e);
                self.report_error(
                    Some(mid),
                    Some(wallpaper_id),
                    &RendererError::new(RendererErrorCode::BackendUnavailable, e.to_string()),
                );
            }
        }

        // 阶段 2：Native 组一次解码共享帧时钟；WebView 逐屏走既有事件链
        let mut native_mids = Vec::new();
        for mid in &targets {
            match self.backends.get(mid).copied() {
                Some(RenderBackend::Native) => native_mids.push(mid.clone()),
                _ => {
                    let wm = self.window_manager.lock().await;
                    if let Err(e) = wm.update_window(mid, wallpaper_id) {
                        log::warn!("[RenderCoordinator] 壁纸更新失败 {}: {}", mid, e);
                    }
                }
            }
        }
        if !native_mids.is_empty() {
            // 只有整组都从 Native 的旧壁纸切到 Native 的新壁纸才淡入淡出；
            // 首帧、跨 backend、同一 id 重复下发一律硬切。
            let crossfade = native_mids.iter().all(|mid| {
                let (old_backend, old_wid) = previous.get(mid).cloned().unwrap_or((None, None));
                old_backend == Some(RenderBackend::Native)
                    && old_wid.is_some()
                    && old_wid != Some(wallpaper_id)
            });
            if let Err(e) = self
                .renderer
                .set_wallpaper(
                    native_mids,
                    descriptor.file_path.clone(),
                    descriptor.kind,
                    descriptor.fit_mode,
                    crossfade,
                )
                .await
            {
                log::warn!("[RenderCoordinator] 原生壁纸更新失败: {}", e);
                self.report_error(
                    Some(&first),
                    Some(wallpaper_id),
                    &RendererError::new(RendererErrorCode::DecodeFailed, e.to_string()),
                );
            }
        }

        // 无论 backend 都记录当前壁纸：WebView 阶段换图也要更新，才不会在
        // 后续切回 Native 时把「同一张图」误判成跨图 fade。
        for mid in &targets {
            self.wallpaper_ids.insert(mid.clone(), wallpaper_id);
        }

        // 主窗口缩略图与 backend 无关，始终通知刷新
        for mid in &targets {
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

        self.refresh_group_geometry();
    }

    /// 清空壁纸：Native 走 renderer，WebView 走现有事件
    pub async fn clear_wallpaper(&mut self, monitor_id: &str, is_sync: bool) {
        let targets = self.resolve_targets(monitor_id, is_sync);
        for mid in targets {
            self.wallpaper_ids.remove(&mid);
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
    ///
    /// Q1：extend 下 Native 渲染固定用 Fill 语义、WebView 页面的 extend 分支也忽略
    /// fitMode，两侧的视觉保持一致；这里仍保存 fitMode，切回 independent 时立即生效。
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

    /// 壁纸变更的目标集合：同步模式取所有已登记显示器，独立模式仅指定显示器
    fn resolve_targets(&self, monitor_id: &str, is_sync: bool) -> Vec<String> {
        if !is_sync {
            return vec![monitor_id.to_string()];
        }
        let mut ids: Vec<String> = self.rects.keys().cloned().collect();
        for id in self.backends.keys() {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        ids.sort();
        ids
    }

    fn native_targets(&self) -> Vec<String> {
        self.backends
            .iter()
            .filter(|(_, backend)| **backend == RenderBackend::Native)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// 重算整组 extend 视口。任何一屏几何 / backend / display_mode 变化都必须调用，
    /// 因为 union 是全组共享的，只更新变化那一屏会让其它屏沿用旧 offset。
    fn refresh_group_geometry(&mut self) {
        let monitors: Vec<(String, MonitorRect)> = self
            .rects
            .iter()
            .map(|(id, rect)| (id.clone(), *rect))
            .collect();

        let ready = !self.backends.is_empty();
        let all_native = ready
            && self
                .backends
                .values()
                .all(|backend| *backend == RenderBackend::Native);
        let extend_active = self.display_mode == DisplayMode::Extend && all_native;

        let viewports: Vec<(String, Option<ExtendViewport>)> = if extend_active {
            match group_viewports(&monitors) {
                Some(group) => {
                    self.degraded_logged = false;
                    group
                        .into_iter()
                        .map(|(id, viewport)| (id, Some(viewport)))
                        .collect()
                }
                None => monitors.iter().map(|(id, _)| (id.clone(), None)).collect(),
            }
        } else {
            // D2：组内混有 WebView backend 时降级为逐屏独立渲染，避免两套几何在接缝冲突
            if self.display_mode == DisplayMode::Extend && ready && !self.degraded_logged {
                log::warn!(
                    "[RenderCoordinator] extend 组内存在非原生渲染目标，已降级为逐屏独立显示"
                );
                self.report_degraded();
                self.degraded_logged = true;
            }
            monitors.iter().map(|(id, _)| (id.clone(), None)).collect()
        };

        if let Err(e) = self.renderer.set_group_geometry(viewports) {
            log::warn!("[RenderCoordinator] 全组几何更新失败: {}", e);
        }
    }

    fn report_degraded(&self) {
        let payload = RendererErrorPayload {
            monitor_id: None,
            wallpaper_id: None,
            code: CODE_EXTEND_DEGRADED.to_string(),
            message: "跨屏拼接需要整组使用原生渲染，当前已降级为逐屏独立显示".to_string(),
        };
        if let Err(e) = self.app_handle.typed_emit(&payload) {
            log::warn!("[RenderCoordinator] 降级提示发送失败: {}", e);
        }
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

    /// 只负责 backend 切换（销毁旧目标 / 建新目标），壁纸内容由调用方组级下发。
    async fn apply_backend(&mut self, monitor_id: &str, descriptor: &Descriptor) -> Result<()> {
        let previous = self.backends.get(monitor_id).copied();

        match descriptor.backend {
            RenderBackend::Native => {
                if previous != Some(RenderBackend::Native) {
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
                    self.renderer
                        .ensure_monitor(monitor_id.to_string(), rect)
                        .await?;
                }
            }
            RenderBackend::WebView => {
                if previous != Some(RenderBackend::WebView) {
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
