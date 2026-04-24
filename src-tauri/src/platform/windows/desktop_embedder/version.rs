//! Windows 版本检测
//!
//! 使用 ntdll.dll 的 RtlGetVersion 获取真实 Build Number，
//! 不受 manifest 兼容性限制影响。
//! Build >= 26100 判定为 24H2+。

use log::{info, warn};

use super::encode_wide;

/// 检测当前 Windows 版本是否为 24H2+（Build >= 26100）
pub(super) fn is_win11_24h2_or_later() -> bool {
    use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;

    unsafe {
        let mut osvi: OSVERSIONINFOW = std::mem::zeroed();
        osvi.dwOSVersionInfoSize = std::mem::size_of::<OSVERSIONINFOW>() as u32;

        type RtlGetVersionFn = unsafe extern "system" fn(*mut OSVERSIONINFOW) -> i32;

        let ntdll = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(
            encode_wide("ntdll.dll\0").as_ptr(),
        );
        if ntdll == std::ptr::null_mut() {
            warn!("无法获取 ntdll.dll 句柄，降级到经典路径");
            return false;
        }

        let proc = windows_sys::Win32::System::LibraryLoader::GetProcAddress(
            ntdll,
            b"RtlGetVersion\0".as_ptr(),
        );
        if proc.is_none() {
            warn!("无法获取 RtlGetVersion，降级到经典路径");
            return false;
        }

        let rtl_get_version: RtlGetVersionFn = std::mem::transmute(proc);
        let status = rtl_get_version(&mut osvi as *mut OSVERSIONINFOW);

        if status == 0 {
            let build = osvi.dwBuildNumber;
            let is_24h2 = build >= 26100;
            info!(
                "Windows 版本: {}.{}.{} → {}",
                osvi.dwMajorVersion,
                osvi.dwMinorVersion,
                build,
                if is_24h2 { "24H2+ (Modern)" } else { "Legacy (Classic)" }
            );
            is_24h2
        } else {
            warn!("RtlGetVersion 失败 (status={})，降级到经典路径", status);
            false
        }
    }
}
