//! 视频播放：常驻 worker（worker.rs）。
//!
//! 文件内部同时承载 `VideoPlayback`（renderer 侧持有的播放状态）与 worker 侧
//! 的 `VideoSession` / `WorkerGpu`，二者通过 `WorkerCommand` 通信、耦合紧密，
//! 暂不进一步拆文件。

mod worker;

pub(crate) use worker::{
    shutdown as mf_shutdown, startup as mf_startup, VideoFrameSnapshot, VideoIdentity,
    VideoPlayback, VideoWorkerHost, PAUSE_FADE, PAUSE_FULLSCREEN,
};
