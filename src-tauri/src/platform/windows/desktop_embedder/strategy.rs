//! 嵌入策略接口与共享类型
//!
//! 定义 `EmbedStrategy` trait 和 `MonitorRect` 值对象，
//! 以及策略选择工厂函数 `select_strategy()`。

use anyhow::Result;
use windows_sys::Win32::Foundation::HWND;

use super::legacy::LegacyStrategy;
use super::modern::ModernStrategy;
use super::version::is_win11_24h2_or_later;

/// 桌面嵌入策略接口
///
/// 不同 Windows 版本的嵌入行为差异通过策略模式封装，
/// 调用方无需关心版本细节。
pub(crate) trait EmbedStrategy {
    /// 策略名称（用于日志）
    fn name(&self) -> &'static str;

    /// 查找目标 WorkerW 窗口
    fn find_workerw(&self, progman: HWND) -> Result<HWND>;

    /// 执行嵌入操作（SetParent + 平台特定的样式修复）
    fn embed(&self, hwnd: HWND, workerw: HWND, rect: MonitorRect) -> Result<()>;
}

/// 显示器矩形区域（虚拟桌面坐标 + 物理分辨率）
#[derive(Debug, Clone, Copy)]
pub(crate) struct MonitorRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// 根据 Windows 版本选择嵌入策略
pub(super) fn select_strategy() -> Box<dyn EmbedStrategy> {
    if is_win11_24h2_or_later() {
        Box::new(ModernStrategy)
    } else {
        Box::new(LegacyStrategy)
    }
}
