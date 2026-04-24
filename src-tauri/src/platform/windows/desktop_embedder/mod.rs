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
    FindWindowW, SendMessageTimeoutW, SetParent, SMTO_NORMAL,
};

use self::strategy::{select_strategy, MonitorRect};

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
pub fn embed_in_desktop(
    hwnd: isize,
    monitor_x: i32,
    monitor_y: i32,
    monitor_width: i32,
    monitor_height: i32,
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
        SendMessageTimeoutW(progman, 0x052C, 0, 0, SMTO_NORMAL, 1000, &mut _result as *mut usize);

        // 3. 选择嵌入策略
        let strategy = select_strategy();
        info!("使用嵌入策略: {}", strategy.name());

        // 4. 查找 WorkerW
        let workerw = strategy
            .find_workerw(progman)
            .context("WorkerW 查找失败")?;

        // 5. 执行嵌入
        strategy
            .embed(hwnd as HWND, workerw, rect)
            .context("嵌入操作失败")?;

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
pub fn unembed_from_desktop(hwnd: isize) {
    unsafe {
        SetParent(hwnd as HWND, std::ptr::null_mut());
        info!("已解除嵌入: HWND=0x{:X}", hwnd);
    }
}
