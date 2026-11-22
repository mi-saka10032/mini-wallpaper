//! 平台适配层
//!
//! 按操作系统分层：`windows` 仅在 Windows 目标下编译，承载窗口嵌入等平台
//! 相关实现；`wallpaper_renderer` 是跨平台的渲染契约与 backend 选择逻辑；
//! `global_shortcut` / `tray` 为各平台共用的能力封装。

#[cfg(target_os = "windows")]
pub mod windows;

pub mod global_shortcut;
pub mod tray;
pub mod wallpaper_renderer;
