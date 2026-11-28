//! Windows 系统回收站删除（`SHFileOperationW`）。
//!
//! 只负责把源文件移入回收站，不碰数据库。调用方只有在返回 Ok 后才硬删
//! wallpaper 记录，避免「文件还在但记录没了」导致下次扫描重新收录。

use std::iter::once;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use anyhow::{bail, Context, Result};
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    SHFileOperationW, FILEOPERATION_FLAGS, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_SILENT,
    FO_DELETE, SHFILEOPSTRUCTW,
};

/// 把单个文件移入系统回收站（可从回收站还原）。
///
/// - `pFrom` 必须是**双 NUL 结尾**的 UTF-16 字符串，`wide` 必须在调用期间存活；
/// - `FOF_ALLOWUNDO` 是核心：不加就是永久删除；
/// - `FOF_NOCONFIRMATION | FOF_SILENT` 抑制系统确认/进度弹窗——应用内已经确认过；
/// - 返回码 0 且 `fAnyOperationsAborted == 0` 才算成功，再做一次存在性校验。
///
/// 该函数会阻塞（Shell 可能等待资源管理器响应），调用方必须放进 `spawn_blocking`。
pub fn recycle_file(path: &Path) -> Result<()> {
    let absolute = path
        .canonicalize()
        .with_context(|| format!("解析待删除文件失败: {path:?}"))?;

    // SHFILEOPSTRUCTW.pFrom 要求 double-null-terminated 的 UTF-16 缓冲区。
    let mut wide: Vec<u16> = absolute
        .as_os_str()
        .encode_wide()
        .chain(once(0))
        .chain(once(0))
        .collect();

    let flags: FILEOPERATION_FLAGS = FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT;
    let mut op = SHFILEOPSTRUCTW {
        hwnd: HWND(std::ptr::null_mut()),
        wFunc: FO_DELETE,
        pFrom: PCWSTR(wide.as_mut_ptr()),
        pTo: PCWSTR::null(),
        // windows crate 的常量是 u32 newtype，而结构体字段是 WORD(u16)
        fFlags: flags.0 as u16,
        ..Default::default()
    };

    // SAFETY: pFrom 指向本函数栈上存活的 double-null 结尾缓冲区；
    // pTo / hwnd 为空；结构体其余字段由 Default 清零，无输出指针需要预分配。
    let code = unsafe { SHFileOperationW(&mut op) };
    if code != 0 {
        bail!(
            "SHFileOperationW 失败: code={code}, path={}",
            absolute.display()
        );
    }
    if op.fAnyOperationsAborted.0 != 0 {
        bail!("回收站操作被中止: {}", absolute.display());
    }
    if absolute.exists() {
        bail!("回收站操作返回成功但文件仍存在: {}", absolute.display());
    }

    Ok(())
}
