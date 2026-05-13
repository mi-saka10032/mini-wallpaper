//! Toast 通知窗口管理器
//!
//! 负责创建、销毁、重排右下角独立 Toast 通知窗口。
//! 每次快捷键操作触发一个独立 Toast 窗口，支持：
//! - 多个 Toast 并存（Vec 管理 label）
//! - 关闭后重排下移（方案 B）
//! - 最大并发数限制（超出时挤掉最早的）
//! - duration 超时自动销毁
//!
//! ## 窗口属性
//! - always-on-top、skip-taskbar、no-focus、transparent、无边框
//! - 出现在主显示器右下角，多个 Toast 从下往上堆叠
//!
//! ## Vec 索引与位置关系
//! - Vec 索引越大 → 越新 → offsetY 越小（越靠近底部/任务栏）
//! - 即新 Toast 出现在最底部，旧的往上推

use log::{info, warn};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// 单个 Toast 条目
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ToastEntry {
    /// 窗口 label（唯一标识）
    pub label: String,
    /// 动作类型（如 "next"、"togglePause"）
    pub action: String,
    /// 显示消息
    pub message: String,
}

/// Toast 窗口尺寸与布局常量
const TOAST_WIDTH: u32 = 320;
const TOAST_HEIGHT: u32 = 72;
const TOAST_MARGIN_RIGHT: i32 = 16;
const TOAST_MARGIN_BOTTOM: i32 = 16;
const TOAST_GAP: i32 = 8;
/// 最大并发 Toast 数量
const MAX_TOASTS: usize = 4;
/// Toast 自动关闭时间（毫秒）
const TOAST_DURATION_MS: u64 = 3000;

/// Toast 通知窗口管理器
pub struct ToastManager {
    /// Tauri 应用句柄
    app_handle: AppHandle,
    /// 当前存活的 Toast 条目列表（索引越大越新，越靠近底部）
    toasts: Vec<ToastEntry>,
    /// 自增计数器，用于生成唯一 label
    counter: u64,
}

impl ToastManager {
    /// 构造 ToastManager
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            app_handle,
            toasts: Vec::new(),
            counter: 0,
        }
    }

    /// 创建一个新的 Toast 通知窗口
    ///
    /// 流程：
    /// 1. 如果已达最大并发数，销毁最早的 Toast
    /// 2. 生成唯一 label，创建 WebviewWindow
    /// 3. 加入 Vec 末尾（最新 = 最底部）
    /// 4. 重新计算所有存活窗口的位置
    /// 5. spawn 定时器，duration 后自动销毁
    pub fn show_toast(&mut self, action: &str, message: &str) {
        // 超出最大数量时，移除最早的
        while self.toasts.len() >= MAX_TOASTS {
            let oldest = self.toasts.remove(0);
            self.destroy_window(&oldest.label);
        }

        // 生成唯一 label
        self.counter += 1;
        let label = format!("toast-{}", self.counter);

        // 构建 URL（通过 query 传参给前端）
        let encoded_message = urlencoding::encode(message);
        let url = format!(
            "/toast?action={}&message={}&label={}",
            action, encoded_message, label
        );

        // 计算初始位置（先放到屏幕外，创建后统一重排）
        let (base_x, base_y) = self.get_base_position();
        let index = self.toasts.len(); // 新 Toast 将在末尾
        let y = base_y - ((index as i32 + 1) * (TOAST_HEIGHT as i32 + TOAST_GAP));

        // 创建窗口
        match WebviewWindowBuilder::new(
            &self.app_handle,
            &label,
            WebviewUrl::App(url.into()),
        )
        .title("Toast")
        .decorations(false)
        .skip_taskbar(true)
        .transparent(true)
        .resizable(false)
        .visible(false)
        .always_on_top(true)
        .focused(false)
        .position(base_x as f64, y as f64)
        .inner_size(TOAST_WIDTH as f64, TOAST_HEIGHT as f64)
        .build()
        {
            Ok(window) => {
                let _ = window.show();
                info!(
                    "[ToastManager] 创建 Toast 窗口: label='{}', action='{}', pos=({}, {})",
                    label, action, base_x, y
                );
            }
            Err(e) => {
                warn!("[ToastManager] 创建 Toast 窗口失败: {}", e);
                return;
            }
        }

        // 加入管理列表
        self.toasts.push(ToastEntry {
            label: label.clone(),
            action: action.to_string(),
            message: message.to_string(),
        });

        // 重排所有窗口位置
        self.recalculate_positions();

        // spawn 定时器：duration 后自动关闭
        let app_handle = self.app_handle.clone();
        let toast_label = label.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(TOAST_DURATION_MS)).await;

            // 检查窗口是否仍然存在（可能已被用户手动关闭）
            if app_handle.get_webview_window(&toast_label).is_some() {
                // 通过 command 路径关闭，确保 Vec 状态同步
                // 这里直接操作 state 来关闭
                if let Some(manager) = app_handle.try_state::<std::sync::Arc<tokio::sync::Mutex<ToastManager>>>() {
                    let mut mgr = manager.lock().await;
                    mgr.close_toast(&toast_label);
                }
            }
        });
    }

    /// 关闭指定 label 的 Toast 窗口
    ///
    /// 由前端点击 close 按钮或 duration 超时调用。
    /// 流程：从 Vec 移除 → 销毁窗口 → 重排剩余窗口位置（方案 B）
    pub fn close_toast(&mut self, label: &str) {
        // 从 Vec 中移除
        let removed = self.toasts.iter().position(|t| t.label == label);
        if let Some(idx) = removed {
            self.toasts.remove(idx);
            self.destroy_window(label);
            // 方案 B：重排剩余窗口位置（下移填补空位）
            self.recalculate_positions();
            info!("[ToastManager] 关闭 Toast: label='{}', 剩余 {} 个", label, self.toasts.len());
        } else {
            // 可能已经被 duration 定时器关闭了，尝试直接销毁窗口
            self.destroy_window(label);
        }
    }

    /// 销毁所有 Toast 窗口（应用退出时调用）
    pub fn destroy_all(&mut self) {
        let labels: Vec<String> = self.toasts.iter().map(|t| t.label.clone()).collect();
        for label in &labels {
            self.destroy_window(label);
        }
        self.toasts.clear();
    }

    /// 重新计算所有存活 Toast 窗口的位置（方案 B 核心）
    ///
    /// 位置规则：
    /// - Vec 索引越大（越新）→ 越靠近底部
    /// - 最新的 Toast 在最底部，旧的往上堆叠
    /// - 关闭中间某个后，上方的 Toast 下移填补
    fn recalculate_positions(&self) {
        let (base_x, base_y) = self.get_base_position();
        let total = self.toasts.len();

        for (i, entry) in self.toasts.iter().enumerate() {
            // i=0 是最旧的（最上面），i=total-1 是最新的（最下面）
            // 从底部往上计算：最新的距离底部最近
            let distance_from_bottom = (total - 1 - i) as i32;
            let y = base_y - ((distance_from_bottom + 1) * (TOAST_HEIGHT as i32 + TOAST_GAP));

            if let Some(window) = self.app_handle.get_webview_window(&entry.label) {
                let _ = window.set_position(tauri::PhysicalPosition::new(base_x, y));
            }
        }
    }

    /// 获取 Toast 窗口的基准位置（主显示器右下角）
    ///
    /// 返回 (base_x, base_y)：
    /// - base_x: 屏幕右边缘 - Toast 宽度 - 右边距
    /// - base_y: 屏幕底部边缘 - 底部边距（任务栏上方）
    fn get_base_position(&self) -> (i32, i32) {
        // 尝试获取主显示器信息
        if let Some(monitor) = self.app_handle.primary_monitor().ok().flatten() {
            let size = monitor.size();
            let position = monitor.position();
            let scale = monitor.scale_factor();

            // 使用物理像素计算
            let screen_width = size.width as i32;
            let screen_height = size.height as i32;
            let monitor_x = position.x;
            let monitor_y = position.y;

            let base_x = monitor_x + screen_width
                - (TOAST_WIDTH as f64 * scale) as i32
                - (TOAST_MARGIN_RIGHT as f64 * scale) as i32;
            let base_y = monitor_y + screen_height
                - (TOAST_MARGIN_BOTTOM as f64 * scale) as i32;

            (base_x, base_y)
        } else {
            // 回退：假设 1920x1080 主显示器
            let base_x = 1920 - TOAST_WIDTH as i32 - TOAST_MARGIN_RIGHT;
            let base_y = 1080 - TOAST_MARGIN_BOTTOM;
            (base_x, base_y)
        }
    }

    /// 销毁指定 label 的窗口实例
    fn destroy_window(&self, label: &str) {
        if let Some(window) = self.app_handle.get_webview_window(label) {
            let _ = window.close();
        }
    }
}
