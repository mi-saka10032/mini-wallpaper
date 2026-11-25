//! 设备层：与壁纸业务无关的平台基础设施。
//!
//! 只提供线程级 COM 生命周期、原生窗口句柄、设备丢失判定三类能力，
//! 不依赖上层 decode / render，是依赖图的最底层。

mod com_scope;
mod device_lost;
mod window;

pub(crate) use com_scope::ComScope;
pub(crate) use device_lost::is_device_lost;
pub(crate) use window::NativeWindow;
