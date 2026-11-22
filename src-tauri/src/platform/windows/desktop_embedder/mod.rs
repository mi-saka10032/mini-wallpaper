//! Win32 桌面壁纸嵌入模块
//!
//! 核心原理：
//! 1. FindWindow("Progman") 找到桌面窗口
//! 2. SendMessageTimeout(Progman, 0x052C) 触发 explorer 创建 WorkerW
//! 3. 根据 Windows 版本选择不同的嵌入策略（策略模式）：
//!    - 24H2+：WndProc 子类化 + 样式清理 + NC 补偿（ModernStrategy）
//!    - 旧版本：经典 EnumWindows + 简单 SetParent（LegacyStrategy）
//! 4. SetParent(tauri_hwnd, workerw) 将壁纸窗口嵌入桌面层级
//!
//! 模块结构：
//! - `mod.rs`      — 公共 API 入口（embed_in_desktop / unembed_from_desktop）
//! - `strategy.rs` — EmbedStrategy trait + MonitorRect + 策略选择工厂
//! - `modern.rs`   — 24H2+ 嵌入策略实现
//! - `legacy.rs`   — 旧版本嵌入策略实现
//! - `wndproc.rs`  — WndProc 子类化基础设施
//! - `version.rs`  — Windows 版本检测
//! - `workerw.rs`  — 经典 WorkerW 查找（EnumWindows 方案）

mod legacy;
mod modern;
mod strategy;
mod version;
mod wndproc;
mod workerw;

use anyhow::{bail, Context, Result};
use log::info;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowLongPtrW, SendMessageTimeoutW, SetParent, SetWindowLongPtrW,
    SetWindowPos, GWL_EXSTYLE, HWND_TOP, SMTO_NORMAL, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

use self::strategy::{select_strategy, MonitorRect};

/// 调用方身份：WebView 与 Native 共用同一套嵌入策略，但窗口样式前提不同。
///
/// - `WebView`：tao 创建的 WS_POPUP 无边框窗口。存量方案依赖其原有混合样式，
///   SetParent 的语义不能因为原生渲染器而改变，否则等于让一条已验证可用的路径
///   静默换一种窗口模型。
/// - `Native`：我们自己创建的 WS_POPUP 窗口。SetParent 前必须清掉 WS_POPUP，
///   否则与 WS_CHILD 互斥、`GetParent` 返回 0，窗口不会真正挂进 WorkerW。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbedPolicy {
    WebView,
    Native,
}

/// 辅助函数：将 &str 编码为以 null 结尾的 UTF-16 Vec
pub(crate) fn encode_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// 在 Windows 上查找 WorkerW 窗口并将指定 HWND 嵌入桌面层级
///
/// 参数：
/// - `hwnd`: 要嵌入的窗口句柄
/// - `monitor_x`, `monitor_y`: 该显示器在虚拟桌面中的左上角坐标
/// - `monitor_width`, `monitor_height`: 该显示器的物理分辨率
///
/// `policy` 会贯穿到具体策略：Native 与 WebView 共用同一套嵌入流程，但窗口
/// 样式前提不同（见 `EmbedPolicy`）。它是两条渲染路径共存的隔离开关，防止
/// Native 的样式清理破坏已验证可用的 WebView fallback。
pub fn embed_in_desktop(
    hwnd: isize,
    monitor_x: i32,
    monitor_y: i32,
    monitor_width: i32,
    monitor_height: i32,
    policy: EmbedPolicy,
) -> Result<()> {
    let rect = MonitorRect {
        x: monitor_x,
        y: monitor_y,
        width: monitor_width,
        height: monitor_height,
    };

    info!("开始嵌入: HWND=0x{:X}, 显示器区域={:?}", hwnd, rect);

    unsafe {
        // 1. 找到 Progman 窗口
        let progman = FindWindowW(encode_wide("Progman\0").as_ptr(), std::ptr::null());
        if progman == std::ptr::null_mut() {
            bail!("未找到 Progman 窗口");
        }

        // 2. 发送 0x052C 消息触发 WorkerW 创建
        let mut _result: usize = 0;
        SendMessageTimeoutW(
            progman,
            0x052C,
            0,
            0,
            SMTO_NORMAL,
            1000,
            &mut _result as *mut usize,
        );

        // 3. 选择嵌入策略
        let strategy = select_strategy();
        info!("使用嵌入策略: {}", strategy.name());

        // 4. 查找 WorkerW
        let workerw = strategy.find_workerw(progman).context("WorkerW 查找失败")?;

        // 5. 执行嵌入
        strategy
            .embed(hwnd as HWND, workerw, rect, policy)
            .context("嵌入操作失败")?;

        // 6. 显式置顶：最近一次嵌入的窗口必须盖住同一块屏幕区域的旧窗口。
        // Native↔WebView 切换时新旧窗口会短暂同时存在（新建先于旧的销毁，
        // 见 RenderCoordinator::apply_backend），SetParent 之后系统默认的
        // Z-order 没有文档保证，不能依赖它恰好把新窗口放在最上面；
        // 这里用 SWP_NOMOVE | SWP_NOSIZE 只改 Z-order、不动位置尺寸，
        // 确保「新窗口已经挡住旧窗口」这个前提恒成立，新内容画出来之前
        // 新窗口是透明的，用户透过它看到的仍是旧窗口内容，不会露出系统桌面。
        SetWindowPos(
            hwnd as HWND,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );

        // 注意：DWM 圆角裁剪（~1px）已确认无法通过任何窗口级 API 消除，
        // Overscan 方案虽能消除圆角但会导致多屏交接处溢出更明显（色差壁纸下尤为突出）。
        // 权衡后选择接受 1px 圆角溢出——面积更小、更可控。

        info!(
            "嵌入完成: HWND=0x{:X} → WorkerW={:?}, pos=({},{}), size={}x{}",
            hwnd, workerw, rect.x, rect.y, rect.width, rect.height
        );

        Ok(())
    }
}

/// 从桌面层级中移除嵌入的窗口
///
/// 必须在 `window.close()` 之前调用：还原 WndProc 需要 HWND 仍然有效，
/// 且紧随其后的 WM_CLOSE / WM_DESTROY 要能回到 tao 自己的窗口过程。
///
/// `SetParent(hwnd, NULL)` 之后、`DestroyWindow` 真正生效之前，窗口会短暂
/// 变回顶层窗口；嵌入阶段（见 `embed_in_desktop` → `ModernStrategy::embed`）
/// 可能已经清掉了 `WS_EX_NOACTIVATE`（Native 窗口成为 WorkerW 子窗口后不再
/// 需要顶层窗口语义），裸露期内这个全屏幕尺寸的窗口有被系统误判为
/// 「前台全屏应用」的风险（`fullscreen_detector` 也从结果侧做了防御，见
/// `monitor_geometry::is_own_wallpaper_window`）。这里补回
/// `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`，从根上让这段裸露期内的窗口
/// 不可被激活/置为前台，双重防御。
pub fn unembed_from_desktop(hwnd: isize) {
    // ModernStrategy 才会子类化，其余情况内部静默跳过
    wndproc::restore_and_unregister(hwnd as HWND);

    unsafe {
        SetParent(hwnd as HWND, std::ptr::null_mut());

        let ex_style = GetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE);
        let guarded_ex_style = ex_style | (WS_EX_NOACTIVATE as isize) | (WS_EX_TOOLWINDOW as isize);
        SetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE, guarded_ex_style);
    }
    info!("已解除嵌入: HWND=0x{:X}", hwnd);
}
