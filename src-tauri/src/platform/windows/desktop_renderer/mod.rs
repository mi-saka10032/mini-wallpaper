//! Windows 原生壁纸渲染器（DirectComposition + D3D11）。
//!
//! 架构：单一 renderer 线程跑消息循环与绘制，单一 video worker 拉帧，
//! 每个 VideoSession 独占自己的 GPU / SourceReader；renderer 默认做本地 NV12 copy，
//! fade 期间同时暂停视频时钟与音频。

// renderer 线程的 COM 生命周期守卫（RAII，与渲染逻辑无关的线程基础设施）。
mod com_scope;
// 独立音频线程：MF 拉 PCM 走 WASAPI 输出，与视频时钟低频校准。
mod audio;
// 图片 / GIF / 动态 WebP 的纯解码与纹理上传（无状态函数）。
mod image;
// 图片一次性解码的调度、去重、generation 过期与结果回收（M22 review 后从 renderer 抽出）。
mod image_worker;
// 渲染线程：命令分发、帧推进、视频暂停 / 音频资格裁决与退出清理。
mod renderer;
// 渲染表面：壁纸三层状态、crossfade、纹理 / SRV / RTV 与 present。
mod surface;
// 视频播放：VideoPlayback / keyed mutex 共享纹理 / 单 worker 调度。
mod video;
// 壁纸原生窗口：创建、嵌入样式归一与销毁；消息泵实际在 renderer 线程。
mod window;

pub use renderer::DesktopRenderer;

/// renderer 关键命令（EnsureMonitor / SetWallpaper）的回执通道。
///
/// 视频与图片链路共用：前者在同步打开后直接回，后者等一次性解码 worker 完成后回。
/// 放在模块根而非 renderer.rs，是为了让 image_worker 不必反向依赖渲染线程的类型。
pub(super) type Ack = tokio::sync::oneshot::Sender<Result<(), String>>;

/// 给单个等待方回执；接收端已退出时静默忽略（渲染线程本就要退出，不应 panic）。
pub(super) fn send_ack(ack: Option<Ack>, result: Result<(), String>) {
    if let Some(tx) = ack {
        let _ = tx.send(result);
    }
}

/// 给一组等待方回同一个结果（Ack 是 oneshot，逐个 send）。
pub(super) fn send_acks(acks: Vec<Ack>, result: Result<(), String>) {
    for ack in acks {
        send_ack(Some(ack), result.clone());
    }
}
