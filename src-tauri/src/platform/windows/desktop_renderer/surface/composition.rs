//! DirectComposition swapchain 的创建与持有。
//!
//! 几个 COM 对象必须一起存活，否则 visual 失效、桌面合成树取不到内容。
//! premultiplied alpha + FLIP_SEQUENTIAL 是 24H2 正确采样壁纸窗口的前提。

use anyhow::Context;
use anyhow::Result;
use windows::core::{IUnknown, Interface};
use windows::Win32::Foundation::{FALSE, HWND};
use windows::Win32::Graphics::Direct3D11::ID3D11Device;
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, IDXGIFactory2, IDXGIOutput, IDXGISwapChain1, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
    DXGI_USAGE_RENDER_TARGET_OUTPUT,
};

/// DirectComposition 合成链：swapchain 的内容经 visual → target → 壁纸 HWND。
///
/// 四个 COM 对象必须一起存活（字段只持有引用，不直接读取），缺任一个都会让
/// visual 失效、桌面合成树取不到壁纸内容。
pub(super) struct CompositionSwapChain {
    /// 渲染目标：swapchain 的 backbuffer 由 RenderSurface 绑定 RTV 并绘制。
    pub(crate) swap_chain: IDXGISwapChain1,
    /// DirectComposition 设备：创建 target / visual 并 `Commit()` 提交合成树变更。
    pub(crate) device: IDCompositionDevice,
    /// 合成目标：绑定壁纸 HWND，是合成树挂到窗口的锚点。
    pub(crate) target: IDCompositionTarget,
    /// 合成视觉节点：内容设为 swapchain，并作为 target 的根节点。
    pub(crate) visual: IDCompositionVisual,
}

/// 建 flip model swapchain，并用 DirectComposition visual 挂到壁纸 HWND。
/// premultiplied alpha + FLIP_SEQUENTIAL 是 24H2 桌面合成树正确采样该窗口的前提。
pub(super) fn create_composition_swap_chain(
    device: &ID3D11Device,
    hwnd: HWND,
    width: u32,
    height: u32,
) -> Result<CompositionSwapChain> {
    // 1. 从 D3D11 device 沿 DXGI 链拿到 factory：swapchain 必须由创建 device 的
    //    同一 adapter 上的 factory 创建，否则跨 adapter 会导致 Present 失败。
    let dxgi_device: IDXGIDevice = device.cast()?;
    let adapter = unsafe { dxgi_device.GetAdapter()? };
    let factory: IDXGIFactory2 = unsafe { adapter.GetParent()? };
    // swapchain 创建要的是 IUnknown 形式的 device，这里再转一次接口。
    let device_unknown: IUnknown = device.cast()?;

    // 2. swapchain 描述：BGRA8 UNORM + premultiplied alpha + flip model。
    //    双缓冲 + 无 MSAA；24H2 桌面合成树只按这套配置正确采样壁纸窗口。
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: width,
        Height: height,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        Stereo: FALSE,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        Scaling: DXGI_SCALING_STRETCH,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
        Flags: 0,
    };

    // 3. 创建 composition swapchain：不绑定窗口，稍后由 DirectComposition visual 挂载。
    let swap_chain: IDXGISwapChain1 = unsafe {
        factory.CreateSwapChainForComposition(&device_unknown, &desc, None::<&IDXGIOutput>)?
    };

    // 4. DirectComposition 合成树：device → target(绑 HWND) → visual。
    let composition_device: IDCompositionDevice =
        unsafe { DCompositionCreateDevice(&dxgi_device)? };
    // true = 独占该 HWND 的合成，避免与其它 DirectComposition 客户端冲突。
    let composition_target = unsafe { composition_device.CreateTargetForHwnd(hwnd, true)? };
    let composition_visual = unsafe { composition_device.CreateVisual()? };

    // 5. 把 swapchain 设为 visual 内容、visual 设为 target 根节点，再一次性提交变更。
    //    swapchain 要先转成 IUnknown 才能作为 SetContent 的通用内容源。
    let content: IUnknown = swap_chain.cast()?;
    unsafe {
        composition_visual.SetContent(&content)?;
        composition_target.SetRoot(&composition_visual)?;
        composition_device.Commit()?;
    }

    Ok(CompositionSwapChain {
        swap_chain,
        device: composition_device,
        target: composition_target,
        visual: composition_visual,
    })
}
