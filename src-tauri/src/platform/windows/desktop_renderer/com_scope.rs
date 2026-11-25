//! renderer 线程的 COM 生命周期守卫（RAII）
//!
//! Windows COM 的初始化是**线程级**的：每个使用 COM 的线程都要自己
//! `CoInitializeEx`，并在退出前 `CoUninitialize` 成对释放。renderer 线程跑
//! DirectComposition / D3D11 / Media Foundation / Win32 消息泵，全是 COM API，
//! 且主循环有多个退出路径（Shutdown、初始化失败提前返回、panic），手写反初始化
//! 容易漏。把生命周期绑到栈变量的 `Drop` 上即可保证成对。
//!
//! 独立成模块的原因：这是与"壁纸渲染逻辑"无关的**线程基础设施**，放在
//! `renderer.rs` 里会和业务代码混在一起；同目录的 `audio.rs` 也有一份同模式
//! 的守卫（MTA，因为音频线程不跑消息泵），两者刻意不合并、各自按线程模型选择。

use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

/// COM 初始化守卫；`should_uninitialize` 记录本次调用是否真的取得了 COM 所有权。
pub(super) struct ComScope {
    should_uninitialize: bool,
}

impl ComScope {
    /// 以 STA 初始化当前线程的 COM。
    ///
    /// renderer 线程跑 Win32 消息泵与窗口，UI 线程惯例用 STA（`APARTMENTTHREADED`）。
    /// 只有「本次调用成功」才置 `should_uninitialize = true`：`RPC_E_CHANGED_MODE`
    /// 表示线程已被别处以不同并发模型初始化、其它 HRESULT 失败表示初始化未成功，
    /// 这两种情况都不能反初始化，否则会误释放别人的 COM 状态。
    pub(super) fn init() -> Self {
        unsafe {
            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            if hr == RPC_E_CHANGED_MODE {
                log::warn!("[Renderer] COM 已由其它并发模型初始化，跳过 CoUninitialize");
                return Self {
                    should_uninitialize: false,
                };
            }
            if hr.0 < 0 {
                log::error!("[Renderer] CoInitializeEx 失败: 0x{:08X}", hr.0 as u32);
                return Self {
                    should_uninitialize: false,
                };
            }
            // 成功含 S_FALSE（同模型重复初始化、引用计数 +1），都必须由 Drop
            // 调一次 CoUninitialize 才能把引用计数平衡回去。
            Self {
                should_uninitialize: true,
            }
        }
    }
}

impl Drop for ComScope {
    /// 仅在本次 init 取得 COM 所有权时释放，避免动到别人的初始化。
    fn drop(&mut self) {
        if self.should_uninitialize {
            unsafe { CoUninitialize() };
        }
    }
}
