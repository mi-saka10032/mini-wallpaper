import {
  cloneElement,
  isValidElement,
  useState,
  type ComponentProps,
  type FC,
  type ReactNode,
} from "react";
import { AlertTriangle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { cn } from "@/lib/utils";

export interface PopConfirmProps {
  /** 触发器元素，必须能接收 ref（内部使用 asChild 透传） */
  children: ReactNode;
  /** 提示标题，通常一句话说明后果 */
  title: ReactNode;
  /** 可选的补充说明，置于标题下方 */
  description?: ReactNode;
  /** 确认按钮文案 */
  confirmText: string;
  /** 取消按钮文案 */
  cancelText: string;
  /** 确认按钮样式，危险操作用 destructive */
  confirmVariant?: ComponentProps<typeof Button>["variant"];
  /** 点击确认后回调 */
  onConfirm: () => void;
  /** 点击取消/关闭浮窗后回调 */
  onCancel?: () => void;
  /**
   * 为 false 时跳过二次确认，直接执行 onConfirm。
   * 用于「无待确认内容时不该打扰用户」的场景。
   */
  enabled?: boolean;
  /** 浮窗相对触发器的对齐方式 */
  align?: ComponentProps<typeof PopoverContent>["align"];
  /** 浮窗出现的方位 */
  side?: ComponentProps<typeof PopoverContent>["side"];
  /** 是否展示左侧警示图标 */
  showIcon?: boolean;
  className?: string;
}

/**
 * 基于触发按钮就地弹出的轻量二次确认浮窗。
 *
 * 与既有两类组件的分工：
 * - AlertDialog：覆盖全局、带遮罩，用于不可逆且代价高的操作；
 * - Tooltip：hover 弱提示，不承载操作；
 * - PopConfirm：锚定在按钮上的小浮窗，用于代价低但仍需用户点头的操作。
 */
const PopConfirm: FC<PopConfirmProps> = ({
  children,
  title,
  description,
  confirmText,
  cancelText,
  confirmVariant = "default",
  onConfirm,
  onCancel,
  enabled = true,
  align = "end",
  side = "bottom",
  showIcon = true,
  className,
}) => {
  const [open, setOpen] = useState(false);

  // 无需确认时退化为普通按钮：直接把 onConfirm 注入子元素，
  // 不额外包一层容器，避免破坏工具栏的 flex 布局与按钮可访问性
  if (!enabled) {
    if (isValidElement<{ onClick?: () => void }>(children)) {
      return cloneElement(children, { onClick: onConfirm });
    }
    return <>{children}</>;
  }

  const handleConfirm = () => {
    setOpen(false);
    onConfirm();
  };

  const handleCancel = () => {
    setOpen(false);
    onCancel?.();
  };

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) onCancel?.();
      }}
    >
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <PopoverContent
        align={align}
        side={side}
        sideOffset={8}
        className={cn("w-auto min-w-56 max-w-72 p-3", className)}
      >
        <div className="flex gap-2.5">
          {showIcon && (
            <AlertTriangle className="mt-0.5 size-4 shrink-0 text-amber-500" aria-hidden="true" />
          )}
          <div className="min-w-0 flex-1">
            <p className="text-sm font-medium leading-snug text-foreground">{title}</p>
            {description && (
              <p className="mt-1 text-xs leading-relaxed text-muted-foreground">{description}</p>
            )}
          </div>
        </div>
        <div className="mt-3 flex justify-end gap-2">
          <Button variant="ghost" size="sm" className="h-7 px-2.5 text-xs" onClick={handleCancel}>
            {cancelText}
          </Button>
          <Button
            variant={confirmVariant}
            size="sm"
            className="h-7 px-2.5 text-xs"
            onClick={handleConfirm}
          >
            {confirmText}
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  );
};

PopConfirm.displayName = "PopConfirm";

export default PopConfirm;
