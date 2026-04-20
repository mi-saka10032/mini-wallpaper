use tauri::State;

use crate::FullscreenDetectorState;

/// 启停全屏检测器
#[tauri::command]
pub async fn set_fullscreen_detection(
    enabled: bool,
    detector_state: State<'_, FullscreenDetectorState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let mut detector = detector_state.lock().await;
    if enabled {
        if !detector.is_running() {
            detector.start(app_handle);
        }
    } else {
        detector.stop();
    }
    Ok(())
}
