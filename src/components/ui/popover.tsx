import type { ComponentProps } from "react";
import { Popover as PopoverPrimitive } from "radix-ui";

import { cn } from "@/lib/utils";

function Popover({ ...props }: ComponentProps<typeof PopoverPrimitive.Root>) {
  return <PopoverPrimitive.Root data-slot="popover" {...props} />;
}

function PopoverTrigger({ ...props }: ComponentProps<typeof PopoverPrimitive.Trigger>) {
  return <PopoverPrimitive.Trigger data-slot="popover-trigger" {...props} />;
}

/**
 * 当前打开的 DialogContent（Portal 目标）。
 *
 * modal Dialog 会用 RemoveScroll 吞掉 Dialog 之外的 wheel，并给 body 设
 * pointer-events:none 只放行 DialogContent。挂到 body 的弹层因此滚不动、
 * 点不动。改挂 DialogContent 内既落在滚动允许区，又不在其内部的
 * overflow-y-auto 容器里，不会撑出滚动条。
 */
function dialogContentContainer(): HTMLElement | undefined {
  if (typeof document === "undefined") return undefined;
  const nodes = document.querySelectorAll<HTMLElement>('[data-slot="dialog-content"]');
  return nodes.length ? nodes[nodes.length - 1] : undefined;
}

function PopoverContent({
  className,
  align = "center",
  sideOffset = 4,
  container,
  ...props
}: ComponentProps<typeof PopoverPrimitive.Content> & {
  container?: HTMLElement | null;
}) {
  // 无 Dialog 时 container 为 undefined，Radix 走默认的 body。
  const portalContainer = container ?? dialogContentContainer();
  return (
    <PopoverPrimitive.Portal container={portalContainer}>
      <PopoverPrimitive.Content
        data-slot="popover-content"
        align={align}
        sideOffset={sideOffset}
        className={cn(
          "z-50 w-72 origin-(--radix-popover-content-transform-origin) rounded-md border bg-popover p-4 text-popover-foreground shadow-md outline-none data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2",
          className,
        )}
        {...props}
      />
    </PopoverPrimitive.Portal>
  );
}

function PopoverAnchor({ ...props }: ComponentProps<typeof PopoverPrimitive.Anchor>) {
  return <PopoverPrimitive.Anchor data-slot="popover-anchor" {...props} />;
}

export { Popover, PopoverTrigger, PopoverContent, PopoverAnchor };
