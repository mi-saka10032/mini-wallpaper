import { useCallback, useRef, useState } from "react";

export function useShortcutRecorder(updateSetting: (key: string, value: string) => void) {
  const [recordingAction, setRecordingAction] = useState<string | null>(null);
  const [pendingShortcut, setPendingShortcut] = useState<string | null>(null);
  const recorderRef = useRef<HTMLDivElement>(null);
  const pendingRef = useRef<string | null>(null);
  const recordingRef = useRef<string | null>(null);

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

  const handleRecordKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const shortcut = eventToShortcut(e);
      if (shortcut) {
        pendingRef.current = shortcut;
        setPendingShortcut(shortcut);
      }
    },
    [eventToShortcut],
  );

  const handleRecordKeyUp = useCallback(
    (e: React.KeyboardEvent) => {
      e.preventDefault();
      const pending = pendingRef.current;
      const action = recordingRef.current;
      if (pending && action) {
        updateSetting(action, pending);
        pendingRef.current = null;
        recordingRef.current = null;
        setPendingShortcut(null);
        setRecordingAction(null);
      }
    },
    [updateSetting],
  );

  const startRecording = useCallback((settingKey: string) => {
    recordingRef.current = settingKey;
    pendingRef.current = null;
    setRecordingAction(settingKey);
    setPendingShortcut(null);
    requestAnimationFrame(() => {
      recorderRef.current?.focus();
    });
  }, []);

  const resetShortcut = useCallback(
    (settingKey: string, defaultValue: string) => {
      updateSetting(settingKey, defaultValue);
      setRecordingAction(null);
    },
    [updateSetting],
  );

  const cancelRecording = useCallback(() => {
    recordingRef.current = null;
    pendingRef.current = null;
    setRecordingAction(null);
    setPendingShortcut(null);
  }, []);

  /** 格式化快捷键显示 */
  const formatShortcut = useCallback((shortcut: string) => {
    const isMac = navigator.platform.toUpperCase().includes("MAC");
    return shortcut
      .replace("CommandOrControl", isMac ? "⌘" : "Ctrl")
      .replace("Alt", isMac ? "⌥" : "Alt")
      .replace("Shift", isMac ? "⇧" : "Shift");
  }, []);

  return {
    recordingAction,
    pendingShortcut,
    recorderRef,
    handleRecordKeyDown,
    handleRecordKeyUp,
    startRecording,
    resetShortcut,
    cancelRecording,
    formatShortcut,
  };
}
