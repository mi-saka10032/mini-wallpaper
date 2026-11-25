//! 图片解码：纯函数（decode.rs）+ 一次性 worker 调度（worker.rs）。

mod decode;
mod worker;

pub(crate) use decode::decode_and_upload;
pub(crate) use worker::{ImageDecodeQueue, Ready};
