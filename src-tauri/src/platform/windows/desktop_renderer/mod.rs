//! Windows 原生壁纸渲染器（DirectComposition + D3D11）。
//!
//! 架构：单一 renderer 线程跑消息循环与绘制，单一 video worker 拉帧，
//! 每个 VideoSession 独占自己的 GPU / SourceReader；renderer 默认做本地 NV12 copy，
//! fade 期间同时暂停视频时钟与音频。

// 三层结构（依赖单向：device ← render → decode）
// 设备层：COM 生命周期、原生窗口、设备丢失判定（最底层，不依赖上层）
mod device;
// 解码 worker 层：图片一次性解码 + 视频 / 音频常驻 worker，产出帧容器
mod decode;
// 渲染层：SharedDevice 资源聚合 + 每屏 RenderSurface + 管线 / 合成链
mod render;
// 入口 / 编排层：跨三层协调，独立在根目录表达其"不属于任何单一层"的地位
mod renderer;

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
