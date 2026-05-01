import { useEffect } from "react";

/**
 * useInputBlock - 禁用壁纸窗口的所有用户输入事件
 *
 * 壁纸窗口是纯展示层，不需要任何用户交互。
 * 前端禁用所有鼠标/键盘事件 + 后端 WS_EX_TRANSPARENT 穿透，双层保障。
 */
export function useInputBlock() {
  useEffect(() => {
    const blockAll = (e: Event) => {
      e.preventDefault();
      e.stopPropagation();
    };

    const mouseEvents = [
      "contextmenu", "click", "dblclick", "mousedown", "mouseup",
      "mousemove", "mouseover", "mouseout", "mouseenter", "mouseleave",
      "wheel", "auxclick",
    ];
    const keyEvents = ["keydown", "keyup", "keypress"];
    const dragEvents = ["dragover", "dragenter", "dragleave", "drop", "drag", "dragstart", "dragend"];
    const otherEvents = ["selectstart", "copy", "cut", "paste", "focus", "blur"];

    const allEvents = [...mouseEvents, ...keyEvents, ...dragEvents, ...otherEvents];

    for (const evt of allEvents) {
      document.addEventListener(evt, blockAll, true);
    }

    return () => {
      for (const evt of allEvents) {
        document.removeEventListener(evt, blockAll, true);
      }
    };
  }, []);
}
