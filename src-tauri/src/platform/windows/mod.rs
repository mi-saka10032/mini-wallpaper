//! Windows 平台实现
//!
//! - `desktop_embedder`：把壁纸窗口嵌入桌面层级（Progman / WorkerW），按系统
//!   版本选择策略，并区分 Native / WebView 两种窗口样式前提。
//! - `desktop_renderer`：原生壁纸渲染表面。
//! - `monitor_geometry`：显示器几何与多屏虚拟桌面坐标换算。

pub mod desktop_embedder;
pub mod desktop_renderer;
pub mod monitor_geometry;
pub mod monitor_hotplug;
pub mod recycle_bin;
