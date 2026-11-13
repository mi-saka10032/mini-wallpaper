import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/components/ui/toast";
import type { ImportResult } from "@/stores/wallpaperStore";

/**
 * 导入结果反馈
 *
 * 三条导入路径（文件选择器 / 原生拖放 / HTML5 拖拽）共用同一套提示语义，
 * 避免各调用点各写一遍、漏掉某个分支（之前文件选择器完全没有任何提示）。
 *
 * 提示策略：所有计数合并成一条 toast，避免部分成功时堆叠好几条。
 * - 用户取消选择 → 静默，不打扰
 * - 全部导入 → success
 * - 部分导入 → warning（有文件没进来，需要用户注意）
 * - 一个都没导入 → warning，绝不静默
 *
 * @returns notify — 传入 ImportResult 即可；notifyError — 传入异常
 */
export function useImportFeedback() {
  const { t } = useTranslation();

  const notify = useCallback(
    (result: ImportResult) => {
      if (result.cancelled) return;

      const { imported, skipped, rejectedBySize } = result;

      // 一个都没进来且没有任何「被跳过」的说明：文件在过滤阶段就被全部剔除，
      // 此时若不提示，用户只看到 loading 一闪而过，分不清成功还是失败
      if (imported === 0 && skipped === 0 && rejectedBySize === 0) {
        toast.warning(t("main.importNothing"));
        return;
      }

      const parts: string[] = [];
      if (imported > 0) parts.push(t("main.importImported", { count: imported }));
      if (skipped > 0) parts.push(t("main.importSkippedUnsupported", { count: skipped }));
      if (rejectedBySize > 0) parts.push(t("main.importSkippedBySize", { count: rejectedBySize }));

      const message = parts.join(t("main.importSummarySeparator"));

      // 部分导入用 warning：有文件没进来，值得用户注意；全部导入才是纯成功
      if (imported > 0 && skipped === 0 && rejectedBySize === 0) {
        toast.success(message);
      } else {
        toast.warning(message);
      }
    },
    [t],
  );

  const notifyError = useCallback(
    (error: unknown) => {
      console.error("[import] failed", error);
      toast.error(
        t("main.importFailed", { reason: error instanceof Error ? error.message : String(error) }),
      );
    },
    [t],
  );

  return { notify, notifyError };
}
