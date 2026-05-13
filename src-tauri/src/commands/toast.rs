use tauri::State;

use crate::ctx::AppContext;

use super::error::CommandResult;

/// 关闭指定 label 的 Toast 通知窗口
///
/// 由前端 Toast 组件点击关闭按钮时调用。
/// 内部委托 `ToastManager::close_toast` 完成：
/// 从 Vec 移除 → 销毁窗口 → 重排剩余窗口位置（方案 B）。
#[tauri::command]
pub async fn close_toast_window(
    ctx: State<'_, AppContext>,
    label: String,
) -> CommandResult<()> {
    let mut toast_mgr = ctx.toast_manager.lock().await;
    toast_mgr.close_toast(&label);
    Ok(())
}
