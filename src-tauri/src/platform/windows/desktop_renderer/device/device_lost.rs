//! D3D11 / DXGI 设备丢失错误判定。
//!
//! 独立成模块：两类错误（anyhow 包装链 / windows 原生 HRESULT）的提取与判定逻辑
//! 自洽，与具体 surface / device 状态无关，任何渲染环节都可复用。

/// 从两类错误中提取 HRESULT：bind_current_backbuffer 经 anyhow 包装（可能多层 Context），
use windows::Win32::Graphics::Dxgi::{
    DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET, DXGI_ERROR_DRIVER_INTERNAL_ERROR,
};

/// Present 直接返回 windows 原生错误。统一成一个 trait，让 is_device_lost 泛型处理两者。
trait DeviceLostHResult {
    fn lost_hresult(&self) -> Option<windows::core::HRESULT>;
}

impl DeviceLostHResult for anyhow::Error {
    fn lost_hresult(&self) -> Option<windows::core::HRESULT> {
        // 沿 anyhow 错误链找 windows::core::Error，避免依赖具体被哪层 Context 包装。
        self.chain().find_map(|cause| {
            cause
                .downcast_ref::<windows::core::Error>()
                .map(|err| err.code())
        })
    }
}

impl DeviceLostHResult for windows::core::Error {
    fn lost_hresult(&self) -> Option<windows::core::HRESULT> {
        Some(self.code())
    }
}

/// 判断 D3D11 / DXGI 错误是否属于设备丢失。设备丢失时整块 device 上的所有资源
/// 同时失效，必须整组清理 surface 与 SharedDevice，不能只重试单屏。
pub(crate) fn is_device_lost<E: DeviceLostHResult>(err: &E) -> bool {
    err.lost_hresult().is_some_and(|code| {
        code == windows::Win32::Graphics::Dxgi::DXGI_ERROR_DEVICE_REMOVED
            || code == windows::Win32::Graphics::Dxgi::DXGI_ERROR_DEVICE_RESET
            || code == windows::Win32::Graphics::Dxgi::DXGI_ERROR_DRIVER_INTERNAL_ERROR
    })
}
