//! 渲染线程独有的共享 D3D11 上下文与 video worker 宿主。
//!
//! device / context / pipeline 全局只能有一份：extend 组内所有 surface 必须绑定
//! 同一份 SRV。所有 surface 在同一渲染线程绘制，即时上下文的非线程安全约束自动满足。

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::Dxgi::IDXGIAdapter;

use super::pipeline::{create_pipeline, QuadPipeline};
use crate::platform::windows::desktop_renderer::decode::{VideoPlayback, VideoWorkerHost};

pub struct SharedDevice {
    pub(crate) device: ID3D11Device,
    // 即时上下文非线程安全；所有 surface 都在同一渲染线程上用它绘制。
    pub(crate) context: ID3D11DeviceContext,
    pub(crate) pipeline: QuadPipeline,
    // 单个 video worker 服务全部 VideoPlayback；每路播放自带独立 GPU / SourceReader。
    video_worker: VideoWorkerHost,
}

impl SharedDevice {
    /// 暴露 device 供 image worker 在渲染线程之外上传纹理；SRV 不能跨 device 使用，
    /// 故 worker 必须克隆这同一个 device，不能自建。
    pub(crate) fn device(&self) -> &ID3D11Device {
        &self.device
    }

    pub fn new() -> Result<Self> {
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        unsafe {
            D3D11CreateDevice(
                None::<&IDXGIAdapter>,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        let device = device.context("D3D11 device 创建失败")?;
        let context = context.context("D3D11 context 创建失败")?;
        let pipeline = create_pipeline(&device)?;
        let video_worker = VideoWorkerHost::start(&device)?;
        Ok(Self {
            device,
            context,
            pipeline,
            video_worker,
        })
    }

    /// 打开一个流式视频播放状态；复用共享 device / context，不做解码缓存
    /// （视频不缓存全部帧，缓存的是唯一一个正在推进的播放状态）。
    pub fn open_video(&self, path: &Path, volume: u32) -> Result<Arc<Mutex<VideoPlayback>>> {
        let playback = VideoPlayback::open(
            &self.device,
            &self.context,
            &self.video_worker,
            path,
            volume,
        )?;
        Ok(Arc::new(Mutex::new(playback)))
    }
}
