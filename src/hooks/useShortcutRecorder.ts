import { useCallback, useRef, useState } from "react";

/**
 * 快捷键录制 hook - 支持冲突检测
 *
 * 当用户录入的快捷键与其他已配置的快捷键冲突时，
 * 会设置 conflictKey 状态，阻止保存并显示冲突提示。
 */
export function useShortcutRecorder(updateSetting: (key: string, value: string) => void) {
  const [recordingAction, setRecordingAction] = useState<string | null>(null);
  const [pendingShortcut, setPendingShortcut] = useState<string | null>(null);
  const [conflictKey, setConflictKey] = useState<string | null>(null);
  const recorderRef = useRef<HTMLDivElement>(null);
  const pendingRef = useRef<string | null>(null);
  const recordingRef = useRef<string | null>(null);
  /** 当前所有快捷键值映射（用于冲突检测） */
  const allShortcutsRef = useRef<Record<string, string>>({});

  /** 将 KeyboardEvent.code 转为 Tauri 快捷键字符串 */
  const eventToShortcut = useCallback((e: React.KeyboardEvent): string | null => {
    const code = e.code;
    if (["ControlLeft", "ControlRight", "MetaLeft", "MetaRight",
         "AltLeft", "AltRight", "ShiftLeft", "ShiftRight"].includes(code)) return null;
    if (!e.ctrlKey && !e.metaKey && !e.altKey) return null;

    const parts: string[] = [];
    if (e.ctrlKey || e.metaKey) parts.push("CommandOrControl");
    if (e.altKey) parts.push("Alt");
    if (e.shiftKey) parts.push("Shift");

    const codeMap: Record<string, string> = {
      KeyA: "A", KeyB: "B", KeyC: "C", KeyD: "D", KeyE: "E", KeyF: "F",
      KeyG: "G", KeyH: "H", KeyI: "I", KeyJ: "J", KeyK: "K", KeyL: "L",
      KeyM: "M", KeyN: "N", KeyO: "O", KeyP: "P", KeyQ: "Q", KeyR: "R",
      KeyS: "S", KeyT: "T", KeyU: "U", KeyV: "V", KeyW: "W", KeyX: "X",
      KeyY: "Y", KeyZ: "Z",
      Digit0: "0", Digit1: "1", Digit2: "2", Digit3: "3", Digit4: "4",
      Digit5: "5", Digit6: "6", Digit7: "7", Digit8: "8", Digit9: "9",
      F1: "F1", F2: "F2", F3: "F3", F4: "F4", F5: "F5", F6: "F6",
      F7: "F7", F8: "F8", F9: "F9", F10: "F10", F11: "F11", F12: "F12",
      ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right",
      Space: "Space", Escape: "Escape", Enter: "Enter", Backspace: "Backspace",
      Delete: "Delete", Tab: "Tab", Home: "Home", End: "End",
      PageUp: "PageUp", PageDown: "PageDown",
      Minus: "-", Equal: "=", BracketLeft: "[", BracketRight: "]",
      Backslash: "\\", Semicolon: ";", Quote: "'", Comma: ",",
      Period: ".", Slash: "/", Backquote: "`",
    };

    const key = codeMap[code];
    if (!key) return null;
    parts.push(key);

    return parts.join("+");
  }, []);

  /** 检测快捷键冲突：返回冲突的 settingKey 或 null */
  const detectConflict = useCallback((shortcut: string, currentSettingKey: string): string | null => {
    const allShortcuts = allShortcutsRef.current;
    for (const [key, value] of Object.entries(allShortcuts)) {
      if (key === currentSettingKey) continue;
      // 标准化比较（忽略大小写）
      if (value.toLowerCase() === shortcut.toLowerCase()) {
        return key;
      }
    }
    return null;
  }, []);

  const handleRecordKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const shortcut = eventToShortcut(e);
      if (shortcut) {
        pendingRef.current = shortcut;
        setPendingShortcut(shortcut);

        // 检测冲突
        const currentKey = recordingRef.current;
        if (currentKey) {
          const conflict = detectConflict(shortcut, currentKey);
          setConflictKey(conflict);
        }
      }
    },
    [eventToShortcut, detectConflict],
  );

  const handleRecordKeyUp = useCallback(
    (e: React.KeyboardEvent) => {
      e.preventDefault();
      const pending = pendingRef.current;
      const action = recordingRef.current;

      // 如果有冲突，不保存
      if (conflictKey) return;

      if (pending && action) {
        updateSetting(action, pending);
        pendingRef.current = null;
        recordingRef.current = null;
        setPendingShortcut(null);
        setRecordingAction(null);
        setConflictKey(null);
      }
    },
    [updateSetting, conflictKey],
  );

  /** 开始录制，传入当前所有快捷键值用于冲突检测 */
  const startRecording = useCallback((settingKey: string, allShortcuts: Record<string, string>) => {
    recordingRef.current = settingKey;
    pendingRef.current = null;
    allShortcutsRef.current = allShortcuts;
    setRecordingAction(settingKey);
    setPendingShortcut(null);
    setConflictKey(null);
    requestAnimationFrame(() => {
      recorderRef.current?.focus();
    });
  }, []);

  const resetShortcut = useCallback(
    (settingKey: string, defaultValue: string) => {
      updateSetting(settingKey, defaultValue);
      setRecordingAction(null);
      setConflictKey(null);
    },
    [updateSetting],
  );

  const cancelRecording = useCallback(() => {
    recordingRef.current = null;
    pendingRef.current = null;
    setRecordingAction(null);
    setPendingShortcut(null);
    setConflictKey(null);
  }, []);

  /** 格式化快捷键显示 */
  const formatShortcut = useCallback((shortcut: string) => {
    const isMac = navigator.platform.toUpperCase().includes("MAC");
    return shortcut
      .replace("CommandOrControl", isMac ? "⌘" : "Ctrl")
      .replace("CmdOrCtrl", isMac ? "⌘" : "Ctrl")
      .replace("Alt", isMac ? "⌥" : "Alt")
      .replace("Shift", isMac ? "⇧" : "Shift");
  }, []);

  return {
    recordingAction,
    pendingShortcut,
    conflictKey,
    recorderRef,
    handleRecordKeyDown,
    handleRecordKeyUp,
    startRecording,
    resetShortcut,
    cancelRecording,
    formatShortcut,
  };
}
