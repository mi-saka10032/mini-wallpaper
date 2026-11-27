import { COMMANDS } from "@/api/config";
import type { ApplySceneResult, SceneState, SceneSummary } from "@/api/config";
import { invoke } from "@/api/invoke";

/** 当前桌面状态（与快照 payload 同构，供弹窗阅览确认） */
export function getCurrentSceneState(): Promise<SceneState> {
  return invoke(COMMANDS.GET_CURRENT_SCENE_STATE);
}

export function listScenes(): Promise<SceneSummary[]> {
  return invoke(COMMANDS.LIST_SCENES);
}

/** 同名保存为覆盖 */
export function saveScene(name: string): Promise<SceneSummary> {
  return invoke(COMMANDS.SAVE_SCENE, { name });
}

export function renameScene(id: number, name: string): Promise<SceneSummary> {
  return invoke(COMMANDS.RENAME_SCENE, { id, name });
}

export function deleteScene(id: number): Promise<void> {
  return invoke(COMMANDS.DELETE_SCENE, { id });
}

export function applyScene(id: number): Promise<ApplySceneResult> {
  return invoke(COMMANDS.APPLY_SCENE, { id });
}
