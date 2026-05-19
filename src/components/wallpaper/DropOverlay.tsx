import { Upload } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";

/**
 * 拖拽导入全屏蒙层
 *
 * 渲染策略：
 * - `pointer-events-none`：蒙层不拦截拖拽事件，事件由父容器接收
 * - `absolute inset-0`：覆盖父容器，需要父容器有 `position: relative`
 * - 通过 visible 切换透明度，实现淡入淡出过渡
 */
const DropOverlay: React.FC<{ visible: boolean }> = ({ visible }) => {
  const { t } = useTranslation();

  return (
    <div
      className={cn(
        "pointer-events-none absolute inset-0 z-30 flex items-center justify-center transition-opacity duration-200",
        "bg-primary/5 backdrop-blur-sm",
        "border-2 border-dashed border-primary/60 rounded-md",
        visible ? "opacity-100" : "opacity-0",
      )}
      aria-hidden={!visible}
    >
      <div className="flex flex-col items-center gap-3 px-6 py-5 rounded-xl bg-background/80 fluent-shadow">
        <div className="flex size-14 items-center justify-center rounded-full bg-primary/10 text-primary">
          <Upload
            className={cn("size-7 transition-transform duration-200", visible && "animate-bounce")}
          />
        </div>
        <div className="text-center">
          <p className="text-sm font-medium text-primary">{t("main.releaseToImport")}</p>
          <p className="mt-0.5 text-xs text-foreground/55">{t("main.supportedFormats")}</p>
        </div>
      </div>
    </div>
  );
};

export default DropOverlay;
