import { useEffect } from "react";
import { toast } from "@/components/ui/toast";
import { listen, EVENTS } from "@/api/event";
import type { ActionToastPayload } from "@/api/event";

/**
 * 监听后端 action-toast 事件，显示操作反馈 Toast
 *
 * 当用户通过全局快捷键或托盘菜单触发动作时，
 * 后端 dispatch_action 完成后会 emit 此事件，
 * 前端收到后通过 sonner toast 显示短暂反馈。
 */
export function useActionToast() {
  useEffect(() => {
    const unlisten = listen(EVENTS.ACTION_TOAST, (payload: ActionToastPayload) => {
      const { action, message } = payload;

      // 根据动作类型选择 toast 样式
      switch (action) {
        case "next":
        case "prev":
          toast.success(message, { duration: 2000 });
          break;
        case "togglePause":
          toast.info(message, { duration: 2000 });
          break;
        default:
          toast(message, { duration: 2000 });
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);
}
