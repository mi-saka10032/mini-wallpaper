//! 统一的 Command 错误类型
//!
//! 所有 `#[tauri::command]` 函数使用 `CommandResult<T>` 作为返回类型，
//! 内部通过 `From` trait 自动转换 `anyhow::Error` / `String` 等常见错误，
//! 消除 command 层大量重复的 `.map_err(|e| e.to_string())`。

use serde::Serialize;

/// Command 层统一错误类型
///
/// 实现 `Serialize` 以满足 Tauri invoke 协议要求，
/// 实现 `From<anyhow::Error>` / `From<String>` 等以支持 `?` 操作符。
#[derive(Debug)]
pub struct CommandError(String);

impl Serialize for CommandError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl From<anyhow::Error> for CommandError {
    fn from(err: anyhow::Error) -> Self {
        Self(err.to_string())
    }
}

impl From<String> for CommandError {
    fn from(err: String) -> Self {
        Self(err)
    }
}

impl From<&str> for CommandError {
    fn from(err: &str) -> Self {
        Self(err.to_string())
    }
}

impl From<tauri::Error> for CommandError {
    fn from(err: tauri::Error) -> Self {
        Self(err.to_string())
    }
}

/// Command 层统一返回类型别名
pub type CommandResult<T> = Result<T, CommandError>;
