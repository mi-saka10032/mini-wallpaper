use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::Emitter;
use tokio::task::JoinHandle;

/// 全屏状态变更事件 payload
#[derive(Clone, serde::Serialize)]
pub struct FullscreenChangedPayload {
    pub is_fullscreen: bool,
}

/// 全屏检测器：后台轮询检测是否有全屏应用，状态变化时 emit 事件
pub struct FullscreenDetector {
    handle: Option<JoinHandle<()>>,
    enabled: Arc<AtomicBool>,
}

impl FullscreenDetector {
    pub fn new() -> Self {
        Self {
            handle: None,
            enabled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 启动全屏检测轮询
    pub fn start(&mut self, app_handle: tauri::AppHandle) {
        self.stop();
        self.enabled.store(true, Ordering::SeqCst);

        let enabled = self.enabled.clone();

        let handle = tokio::spawn(async move {
            let mut was_fullscreen = false;

            loop {
                if !enabled.load(Ordering::SeqCst) {
                    break;
                }

                let is_fullscreen = check_fullscreen();

                if is_fullscreen != was_fullscreen {
                    if is_fullscreen {
                        println!("[FullscreenDetector] Fullscreen app detected — pausing wallpaper");
                    } else {
                        println!("[FullscreenDetector] Fullscreen app exited — resuming wallpaper");
                    }

                    let _ = app_handle.emit(
                        "fullscreen-changed",
                        FullscreenChangedPayload { is_fullscreen },
                    );
                    was_fullscreen = is_fullscreen;
                }

                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });

        self.handle = Some(handle);
        println!("[FullscreenDetector] Started");
    }

    /// 停止全屏检测
    pub fn stop(&mut self) {
        self.enabled.store(false, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
        println!("[FullscreenDetector] Stopped");
    }

    pub fn is_running(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }
}

/// 平台相关的全屏检测实现
///
/// macOS: 使用 CGWindowListCopyWindowInfo 检查 kCGWindowLayer == 0 的窗口
///        是否占满整个屏幕
/// Windows: 使用前台窗口尺寸与屏幕尺寸比对（TODO: 后续实现）
#[cfg(target_os = "macos")]
fn check_fullscreen() -> bool {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionaryRef;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_graphics::display::{
        kCGNullWindowID, kCGWindowListOptionOnScreenOnly, CGWindowListCopyWindowInfo,
    };

    unsafe {
        let window_list = CGWindowListCopyWindowInfo(
            kCGWindowListOptionOnScreenOnly,
            kCGNullWindowID,
        );
        if window_list.is_null() {
            return false;
        }

        let count = core_foundation::array::CFArray::<CFType>::wrap_under_get_rule(
            window_list as _,
        );

        for i in 0..count.len() {
            let Some(dict) = count.get(i) else { continue };
            let dict_ref: CFDictionaryRef = dict.as_CFTypeRef() as _;

            // 检查 kCGWindowLayer == 0 (普通窗口层)
            let layer_key = CFString::new("kCGWindowLayer");
            let mut layer_val: *const core_foundation::base::CFTypeRef =
                std::ptr::null();
            if core_foundation::dictionary::CFDictionaryGetValueIfPresent(
                dict_ref,
                layer_key.as_CFTypeRef() as _,
                &mut layer_val as *mut _ as _,
            ) == 0
            {
                continue;
            }
            if layer_val.is_null() {
                continue;
            }

            let layer_num =
                CFNumber::wrap_under_get_rule(layer_val as core_foundation::number::CFNumberRef);
            let layer: i32 = layer_num.to_i32().unwrap_or(-1);
            if layer != 0 {
                continue;
            }

            // 检查该窗口的 owner 不是我们自己的应用
            let owner_key = CFString::new("kCGWindowOwnerName");
            let mut owner_val: *const core_foundation::base::CFTypeRef =
                std::ptr::null();
            if core_foundation::dictionary::CFDictionaryGetValueIfPresent(
                dict_ref,
                owner_key.as_CFTypeRef() as _,
                &mut owner_val as *mut _ as _,
            ) != 0
                && !owner_val.is_null()
            {
                let owner =
                    CFString::wrap_under_get_rule(owner_val as core_foundation::string::CFStringRef);
                let name = owner.to_string();
                // 排除自己和 Finder/Desktop 等系统组件
                if name == "mini-wallpaper"
                    || name == "Window Server"
                    || name == "Finder"
                    || name == "Dock"
                {
                    continue;
                }
            }

            // 检查 kCGWindowIsOnscreen == true
            let onscreen_key = CFString::new("kCGWindowIsOnscreen");
            // 如果能找到这个 key 且为 true，再检查 bounds
            // (已经过滤了 OnScreenOnly，这里略过)

            // 检查窗口尺寸是否占满主屏幕
            let bounds_key = CFString::new("kCGWindowBounds");
            let mut bounds_val: *const core_foundation::base::CFTypeRef =
                std::ptr::null();
            if core_foundation::dictionary::CFDictionaryGetValueIfPresent(
                dict_ref,
                bounds_key.as_CFTypeRef() as _,
                &mut bounds_val as *mut _ as _,
            ) == 0
            {
                continue;
            }
            if bounds_val.is_null() {
                continue;
            }

            let bounds_dict: CFDictionaryRef = bounds_val as _;

            let width = get_dict_number(bounds_dict, "Width").unwrap_or(0.0);
            let height = get_dict_number(bounds_dict, "Height").unwrap_or(0.0);

            // 获取主显示器尺寸
            let main_display = core_graphics::display::CGMainDisplayID();
            let screen_w = core_graphics::display::CGDisplayPixelsWide(main_display) as f64;
            let screen_h = core_graphics::display::CGDisplayPixelsHigh(main_display) as f64;

            // 窗口尺寸 >= 屏幕尺寸视为全屏
            if width >= screen_w && height >= screen_h {
                return true;
            }
        }
    }

    false
}

#[cfg(target_os = "macos")]
unsafe fn get_dict_number(dict: core_foundation::dictionary::CFDictionaryRef, key: &str) -> Option<f64> {
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_foundation::base::TCFType;

    let cf_key = CFString::new(key);
    let mut val: *const core_foundation::base::CFTypeRef = std::ptr::null();
    if core_foundation::dictionary::CFDictionaryGetValueIfPresent(
        dict,
        cf_key.as_CFTypeRef() as _,
        &mut val as *mut _ as _,
    ) == 0
    {
        return None;
    }
    if val.is_null() {
        return None;
    }
    let num = CFNumber::wrap_under_get_rule(val as core_foundation::number::CFNumberRef);
    num.to_f64()
}

#[cfg(not(target_os = "macos"))]
fn check_fullscreen() -> bool {
    // Windows/Linux: TODO 后续实现
    // Windows 方案: GetForegroundWindow + GetWindowRect 比对屏幕尺寸
    false
}
