import {
  createContext,
  useCallback,
  useContext,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ComponentProps,
  type CSSProperties,
  type MutableRefObject,
  type RefObject,
} from "react";
import { XIcon } from "lucide-react";
import { Dialog as DialogPrimitive } from "radix-ui";

import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";

/**
 * Dialog 上下文：用于在 Trigger 和 Content 之间共享触发元素引用
 */
interface DialogContextValue {
  triggerRef: RefObject<HTMLElement | null>;
}

const DialogContext = createContext<DialogContextValue>({
  triggerRef: { current: null },
});

function Dialog({ ...props }: ComponentProps<typeof DialogPrimitive.Root>) {
  const triggerRef = useRef<HTMLElement | null>(null);
  const contextValue = useMemo(() => ({ triggerRef }), []);

  return (
    <DialogContext.Provider value={contextValue}>
      <DialogPrimitive.Root data-slot="dialog" {...props} />
    </DialogContext.Provider>
  );
}

function DialogTrigger({
  ref,
  ...props
}: ComponentProps<typeof DialogPrimitive.Trigger>) {
  const { triggerRef } = useContext(DialogContext);

  const composedRef = useCallback(
    (node: HTMLButtonElement | null) => {
      triggerRef.current = node;
      if (typeof ref === "function") {
        ref(node);
      } else if (ref) {
        (ref as MutableRefObject<HTMLButtonElement | null>).current = node;
      }
    },
    [ref, triggerRef],
  );

  return <DialogPrimitive.Trigger data-slot="dialog-trigger" ref={composedRef} {...props} />;
}

function DialogPortal({ ...props }: ComponentProps<typeof DialogPrimitive.Portal>) {
  return <DialogPrimitive.Portal data-slot="dialog-portal" {...props} />;
}

function DialogClose({ ...props }: ComponentProps<typeof DialogPrimitive.Close>) {
  return <DialogPrimitive.Close data-slot="dialog-close" {...props} />;
}

function DialogOverlay({
  className,
  ...props
}: ComponentProps<typeof DialogPrimitive.Overlay>) {
  return (
    <DialogPrimitive.Overlay
      data-slot="dialog-overlay"
      className={cn(
        "fixed inset-0 z-50 bg-black/50 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:animate-in data-[state=open]:fade-in-0",
        className,
      )}
      {...props}
    />
  );
}

/**
 * 计算 trigger 元素中心相对于视口中心的偏移量
 */
function computeTriggerOffset(triggerEl: HTMLElement | null): { x: number; y: number } | null {
  if (!triggerEl) return null;
  const rect = triggerEl.getBoundingClientRect();
  const viewportCenterX = window.innerWidth / 2;
  const viewportCenterY = window.innerHeight / 2;
  const triggerCenterX = rect.left + rect.width / 2;
  const triggerCenterY = rect.top + rect.height / 2;
  return {
    x: Math.round(triggerCenterX - viewportCenterX),
    y: Math.round(triggerCenterY - viewportCenterY),
  };
}

function DialogContent({
  className,
  children,
  showCloseButton = true,
  style,
  ...props
}: ComponentProps<typeof DialogPrimitive.Content> & {
  showCloseButton?: boolean;
}) {
  const { triggerRef } = useContext(DialogContext);
  const [offset, setOffset] = useState<{ x: number; y: number } | null>(null);

  // 在 Dialog 打开时（Content 挂载时）计算 trigger 偏移
  useLayoutEffect(() => {
    setOffset(computeTriggerOffset(triggerRef.current));
  }, [triggerRef]);

  return (
    <DialogPortal data-slot="dialog-portal">
      <DialogOverlay />
      <DialogPrimitive.Content
        data-slot="dialog-content"
        className={cn(
          "fixed top-[50%] left-[50%] z-50 grid w-full max-w-[calc(100%-2rem)] gap-4 rounded-lg border bg-background p-6 shadow-lg outline-none sm:max-w-lg",
          "dialog-animate",
          className,
        )}
        style={{
          "--dialog-offset-x": `${offset?.x ?? 0}px`,
          "--dialog-offset-y": `${offset?.y ?? 0}px`,
          ...style,
        } as CSSProperties}
        {...props}
      >
        {children}
        {showCloseButton && (
          <DialogPrimitive.Close
            data-slot="dialog-close"
            className="absolute top-4 right-4 rounded-xs opacity-70 ring-offset-background transition-opacity hover:opacity-100 focus:ring-2 focus:ring-ring focus:ring-offset-2 focus:outline-hidden disabled:pointer-events-none data-[state=open]:bg-accent data-[state=open]:text-muted-foreground [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4"
          >
            <XIcon />
            <span className="sr-only">Close</span>
          </DialogPrimitive.Close>
        )}
      </DialogPrimitive.Content>
    </DialogPortal>
  );
}

function DialogHeader({ className, ...props }: ComponentProps<"div">) {
  return (
    <div
      data-slot="dialog-header"
      className={cn("flex flex-col gap-2 text-center sm:text-left", className)}
      {...props}
    />
  );
}

function DialogFooter({
  className,
  showCloseButton = false,
  children,
  ...props
}: ComponentProps<"div"> & {
  showCloseButton?: boolean;
}) {
  return (
    <div
      data-slot="dialog-footer"
      className={cn("flex flex-col-reverse gap-2 sm:flex-row sm:justify-end", className)}
      {...props}
    >
      {children}
      {showCloseButton && (
        <DialogPrimitive.Close asChild>
          <Button variant="outline">Close</Button>
        </DialogPrimitive.Close>
      )}
    </div>
  );
}

function DialogTitle({ className, ...props }: ComponentProps<typeof DialogPrimitive.Title>) {
  return (
    <DialogPrimitive.Title
      data-slot="dialog-title"
      className={cn("text-lg leading-none font-semibold", className)}
      {...props}
    />
  );
}

function DialogDescription({
  className,
  ...props
}: ComponentProps<typeof DialogPrimitive.Description>) {
  return (
    <DialogPrimitive.Description
      data-slot="dialog-description"
      className={cn("text-sm text-muted-foreground", className)}
      {...props}
    />
  );
}

export {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogOverlay,
  DialogPortal,
  DialogTitle,
  DialogTrigger,
};