import { useEffect } from "react";

/**
 * 需要拦截的键盘快捷键列表
 *
 * 格式: { ctrl?: boolean, shift?: boolean, alt?: boolean, key: string }
 * key 使用 KeyboardEvent.key 的值（大小写不敏感匹配）
 */
const BLOCKED_SHORTCUTS: { ctrl?: boolean; shift?: boolean; alt?: boolean; key: string }[] = [
  // --- 查找 / 替换 ---
  { ctrl: true, key: "f" },           // Ctrl+F  查找
  { ctrl: true, key: "g" },           // Ctrl+G  查找下一个
  { ctrl: true, shift: true, key: "g" }, // Ctrl+Shift+G 查找上一个
  { ctrl: true, key: "h" },           // Ctrl+H  替换

  // --- 刷新 ---
  { key: "F5" },                       // F5      刷新
  { ctrl: true, key: "r" },           // Ctrl+R  刷新
  { ctrl: true, shift: true, key: "r" }, // Ctrl+Shift+R 强制刷新

  // --- 保存 / 打印 ---
  { ctrl: true, key: "s" },           // Ctrl+S  保存
  { ctrl: true, key: "p" },           // Ctrl+P  打印

  // --- 查看源码 ---
  { ctrl: true, key: "u" },           // Ctrl+U  查看源码

  // --- 缩放 ---
  { ctrl: true, key: "+" },           // Ctrl++  放大
  { ctrl: true, key: "=" },           // Ctrl+=  放大（部分键盘）
  { ctrl: true, key: "-" },           // Ctrl+-  缩小
  { ctrl: true, key: "0" },           // Ctrl+0  重置缩放

  // --- 全选 ---
  { ctrl: true, key: "a" },           // Ctrl+A  全选

  // --- 导航 ---
  { alt: true, key: "ArrowLeft" },    // Alt+←   后退
  { alt: true, key: "ArrowRight" },   // Alt+→   前进

  // --- 功能键 ---
  { key: "F3" },                       // F3      查找下一个
  { key: "F7" },                       // F7      光标浏览模式
];

/**
 * 判断键盘事件是否匹配某条规则
 */
function matchShortcut(
  e: KeyboardEvent,
  rule: { ctrl?: boolean; shift?: boolean; alt?: boolean; key: string },
): boolean {
  const ctrlMatch = rule.ctrl ? (e.ctrlKey || e.metaKey) : !(e.ctrlKey || e.metaKey);
  const shiftMatch = rule.shift ? e.shiftKey : !e.shiftKey;
  const altMatch = rule.alt ? e.altKey : !e.altKey;
  const keyMatch = e.key.toLowerCase() === rule.key.toLowerCase();
  return ctrlMatch && shiftMatch && altMatch && keyMatch;
}

/**
 * 页面安全防护 hook
 *
 * 禁用浏览器默认热键（查找、刷新、保存、打印、缩放等）和右键菜单，
 * 防止桌面应用中出现不符合预期的浏览器行为。
 *
 * 无论 dev 还是 build 均生效。
 */
export function useWebGuard() {
  useEffect(() => {
    /** 拦截键盘快捷键 */
    const onKeyDown = (e: KeyboardEvent) => {
      // 如果焦点在 input / textarea / contenteditable 中，放行部分快捷键
      const target = e.target as HTMLElement;
      const isEditable =
        target.tagName === "INPUT" ||
        target.tagName === "TEXTAREA" ||
        target.isContentEditable;

      for (const rule of BLOCKED_SHORTCUTS) {
        if (matchShortcut(e, rule)) {
          // 在可编辑元素中，放行 Ctrl+A（全选文本）
          if (isEditable && rule.ctrl && rule.key.toLowerCase() === "a") {
            return;
          }
          // 管理模式下放行 Ctrl+A（全选卡片）
          if (rule.ctrl && rule.key.toLowerCase() === "a" && document.body.hasAttribute("data-manage-mode")) {
            return;
          }
          e.preventDefault();
          e.stopPropagation();
          return;
        }
      }
    };

    /** 禁用右键菜单（放行组件自定义 ContextMenu 区域） */
    const onContextMenu = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      // Radix ContextMenuTrigger 标记为 data-slot="context-menu-trigger"
      // 如果点击目标在其内部，则放行，让组件自己处理右键菜单
      if (target.closest?.('[data-slot="context-menu-trigger"]')) {
        return;
      }
      e.preventDefault();
    };

    document.addEventListener("keydown", onKeyDown, true);
    document.addEventListener("contextmenu", onContextMenu, true);

    return () => {
      document.removeEventListener("keydown", onKeyDown, true);
      document.removeEventListener("contextmenu", onContextMenu, true);
    };
  }, []);
}
