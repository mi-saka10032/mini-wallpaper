//! 全局快捷键注册与路由
//!
//! 把"操作系统级别的键盘事件"翻译为 [`Action`]，再委托
//! [`Scheduler::dispatch_action`] 执行。
//!
//! ## 职责
//!
//! - **键位发现**：从 DB 读取用户自定义键位，缺省时回退到 [`DEFAULTS`]
//! - **plugin handler 装载**：在 `tauri::Builder` 阶段为
//!   `tauri_plugin_global_shortcut` 装上统一回调
//! - **应用启动注册**：在 `setup` 阶段批量 register 默认 / 自定义键位
//!
//! ## 与 `commands::shortcut::switch_wallpaper` 的语义切割
//!
//! 主窗口 UI 中"上一张/下一张"按钮调用旧的 `switch_wallpaper` 命令
//! （全屏遍历切换），符合"主动管理多屏"的语义；
//! 而本模块对应 OS 全局键盘事件，走 dispatcher
//! （精确切换 active 屏），符合"用户在桌面工作时只想动眼前那块屏"
//! 的语义。两条路径并存且互不干扰。
//!
//! ## P0 落地范围
//!
//! - ✅ Next / Prev / TogglePause / OpenMain 四组基础切换/控制快捷键
//! - ✅ ToggleFavorite：收藏/取消收藏当前显示壁纸（当前壁纸权威源已由
//!   `monitor_configs.wallpaper_id` 建立）
//! - ⏸️ Quit：仅托盘菜单可达，避免误触退出

use std::sync::Arc;
use std::collections::HashMap;
use std::sync::{Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use log::{info, warn};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{
    Builder as GlobalShortcutBuilder, GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState,
};
use tokio::sync::Mutex;

use crate::ctx::AppContext;
use crate::dto::app_setting_dto::keys as setting_keys;
use crate::runtime::action::Action;
use crate::runtime::Scheduler;
use crate::services::app_setting_service;

// ==================== 默认快捷键定义 ====================

/// 默认快捷键三元组：setting key → 默认 accelerator → Action
struct DefaultShortcut {
    /// 在 `app_settings` 表中的 key（用户改键后持久化到此 key）
    setting_key: &'static str,
    /// 默认快捷键字符串（DB 中查不到时回退使用）
    default_accelerator: &'static str,
    /// 命中后派发的 Action
    action: Action,
}

/// 默认快捷键组（Next / Prev / TogglePause / OpenMain / ToggleFavorite）
///
/// `Quit` 故意不绑定（避免误触退出）。
const DEFAULTS: &[DefaultShortcut] = &[
    DefaultShortcut {
        setting_key: setting_keys::SHORTCUT_NEXT_WALLPAPER,
        default_accelerator: "CmdOrCtrl+Alt+Right",
        action: Action::Next,
    },
    DefaultShortcut {
        setting_key: setting_keys::SHORTCUT_PREV_WALLPAPER,
        default_accelerator: "CmdOrCtrl+Alt+Left",
        action: Action::Prev,
    },
    DefaultShortcut {
        setting_key: setting_keys::SHORTCUT_TOGGLE_PAUSE,
        default_accelerator: "CmdOrCtrl+Alt+Space",
        action: Action::TogglePause,
    },
    DefaultShortcut {
        setting_key: setting_keys::SHORTCUT_OPEN_MAIN,
        default_accelerator: "CmdOrCtrl+Alt+W",
        action: Action::OpenMain,
    },
    DefaultShortcut {
        setting_key: setting_keys::SHORTCUT_TOGGLE_FAVORITE,
        default_accelerator: "CmdOrCtrl+Alt+F",
        action: Action::ToggleFavorite,
    },
];

// ==================== Plugin Builder ====================

/// 构造已装好 handler 的 global-shortcut plugin
///
/// 在 `lib.rs` 的 `tauri::Builder::default()` 链路中替换原先的
/// `Builder::new().build()` 调用。
pub fn build_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    GlobalShortcutBuilder::new()
        .with_handler(|app, shortcut, event| handle_shortcut(app, shortcut, event))
        .build()
}

// ==================== Handler ====================

/// 全局快捷键节流间隔
///
/// 500ms 内同一键位的重复触发会被丢弃，避免快速连按
/// （尤其是 ToggleFavorite）导致后端重复执行与 toast 堆积。
const SHORTCUT_THROTTLE: Duration = Duration::from_millis(500);

/// 每个快捷键上次实际派发的时间戳（进程级）
///
/// key 用 `Shortcut::id()`（稳定 u32），使节流判断得以前置到
/// `resolve_action`（含一次 DB 查询）之前——被节流丢弃的重复按键
/// 因此不再付出反查开销。
fn last_fire_map() -> &'static StdMutex<HashMap<u32, Instant>> {
    static MAP: OnceLock<StdMutex<HashMap<u32, Instant>>> = OnceLock::new();
    MAP.get_or_init(|| StdMutex::new(HashMap::new()))
}

/// 按键位独立节流：允许派发返回 true，被节流丢弃返回 false
fn should_fire(shortcut_id: u32) -> bool {
    let now = Instant::now();
    let mut map = match last_fire_map().lock() {
        Ok(m) => m,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(last) = map.get(&shortcut_id) {
        if now.duration_since(*last) < SHORTCUT_THROTTLE {
            return false;
        }
    }
    map.insert(shortcut_id, now);
    true
}

/// plugin handler 单一入口
fn handle_shortcut(app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    // 仅按下沿派发：避免长按重复触发与抬起噪声
    if event.state() != ShortcutState::Pressed {
        return;
    }

    // 时间节流（前置于 resolve）：短时间高频触发同一键位时丢弃，
    // 既避免重复执行与 toast 堆积，也省去被丢弃按键的 DB 反查开销
    if !should_fire(shortcut.id()) {
        info!(
            "[GlobalShortcut] 键位 {:?} 被节流丢弃（<{}ms）",
            shortcut,
            SHORTCUT_THROTTLE.as_millis()
        );
        return;
    }

    // 反查命中的 Action
    let action = match resolve_action(app, shortcut) {
        Some(a) => a,
        None => {
            warn!(
                "[GlobalShortcut] 未识别的快捷键: {:?}（已注册但未映射）",
                shortcut
            );
            return;
        }
    };

    info!("[GlobalShortcut] 触发动作: {:?}", action);

    // 异步派发，避免阻塞 plugin 事件线程
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let scheduler_state = match app_handle.try_state::<Arc<Mutex<Scheduler>>>() {
            Some(s) => s,
            None => {
                warn!("[GlobalShortcut] Scheduler 尚未注册，丢弃动作 {:?}", action);
                return;
            }
        };
        let scheduler = scheduler_state.inner().clone();
        let mut sched = scheduler.lock().await;
        sched.dispatch_action(action).await;
    });
}

/// 在已注册键位中反查 Action
///
/// 注意：handler 是同步上下文，这里需要 `block_on` 一次 DB 查询。
/// 由于 setting 表只有 ~10 行 + sqlite，开销可忽略；后续若性能敏感
/// 可改造为启动期一次性构建 `HashMap<Shortcut, Action>` 缓存。
fn resolve_action(app: &AppHandle, shortcut: &Shortcut) -> Option<Action> {
    let ctx = app.try_state::<AppContext>()?;
    let db = ctx.db.clone();

    tauri::async_runtime::block_on(async move {
        for def in DEFAULTS {
            let accelerator = current_accelerator(&db, def).await;
            if let Ok(parsed) = accelerator.parse::<Shortcut>() {
                if &parsed == shortcut {
                    return Some(def.action.clone());
                }
            }
        }
        None
    })
}

// ==================== Setup 阶段批量注册 ====================

/// 在 `tauri::Builder::setup` 阶段调用：批量注册所有快捷键
///
/// 流程：遍历 [`DEFAULTS`] → 读取每个 setting_key 的 DB 值（无值则用默认）
/// → 解析为 `Shortcut` → 调用 plugin 的 `register`。
///
/// 容错策略：单个键位失败仅 warn，不中断整体注册。
pub async fn register_default_shortcuts(app: &AppHandle) {
    let ctx = match app.try_state::<AppContext>() {
        Some(c) => c,
        None => {
            warn!("[GlobalShortcut] AppContext 未就绪，跳过快捷键注册");
            return;
        }
    };
    let db = ctx.db.clone();
    let manager = app.global_shortcut();

    for def in DEFAULTS {
        let accelerator = current_accelerator(&db, def).await;

        let shortcut: Shortcut = match accelerator.parse() {
            Ok(s) => s,
            Err(e) => {
                warn!(
                    "[GlobalShortcut] 无法解析键位 '{}'（key={}）: {}",
                    accelerator, def.setting_key, e
                );
                continue;
            }
        };

        match manager.register(shortcut) {
            Ok(()) => info!(
                "[GlobalShortcut] 注册成功: {} -> {:?} ({})",
                accelerator, def.action, def.setting_key
            ),
            Err(e) => warn!(
                "[GlobalShortcut] 注册失败: {} ({}): {}",
                accelerator, def.setting_key, e
            ),
        }
    }
}

/// 键位变更后全组重注册
///
/// 用户在设置面板修改任一快捷键并落库后，由 `set_setting` 的副作用调用。
/// 先注销当前进程注册的全部全局快捷键，再调用 [`register_default_shortcuts`]
/// 重新读取 DB（此时新值已写入）批量注册，从而让新键位即时生效、旧键位失效。
///
/// 之所以整组重注册而非单键增量更新：快捷键变更是低频操作，
/// 全组重注册可彻底规避"旧 accelerator 捕获 / 单键失败回退"的复杂度。
pub async fn reregister_all(app: &AppHandle) {
    let manager = app.global_shortcut();
    if let Err(e) = manager.unregister_all() {
        warn!("[GlobalShortcut] 注销旧键位失败（继续重注册）: {}", e);
    }
    register_default_shortcuts(app).await;
    info!("[GlobalShortcut] 键位变更后已完成全组重注册");
}

// ==================== 内部工具 ====================

/// 获取某个动作当前生效的 accelerator 字符串
///
/// 优先从 DB 读取；若用户从未配置则回退默认值。
async fn current_accelerator(db: &sea_orm::DatabaseConnection, def: &DefaultShortcut) -> String {
    match app_setting_service::get(db, def.setting_key).await {
        Ok(Some(v)) if !v.trim().is_empty() => v,
        _ => def.default_accelerator.to_string(),
    }
}