import type { useDraggable } from "@dnd-kit/core";
import { t } from "@lingui/core/macro";
import { GripVertical, Plus, X } from "lucide-react";
import { createContext, type ReactNode, useContext, useLayoutEffect, useRef } from "react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

export const PaneEditingContext = createContext<{
  editing: boolean;
  title: string;
  drag: Pick<ReturnType<typeof useDraggable>, "attributes" | "listeners" | "setActivatorNodeRef">;
  full: boolean;
  beside: () => void;
  hide: () => void;
} | null>(null);

export function PaneTitleRow({
  children,
  className,
  layoutOnly = false,
}: {
  children: ReactNode;
  className?: string;
  layoutOnly?: boolean;
}) {
  const layout = useContext(PaneEditingContext);
  const active = layout?.editing;
  const ref = useRef<HTMLDivElement>(null);
  const grip = useRef<HTMLButtonElement>(null);
  const setActivator = layout?.drag.setActivatorNodeRef;
  useLayoutEffect(() => {
    if (!active || !ref.current) return;
    setActivator?.(grip.current);
    const siblings: { el: HTMLElement; inert: boolean }[] = [];
    let branch: HTMLElement = ref.current;
    while (branch.parentElement && !branch.classList.contains("live-pane-fit")) {
      for (const el of branch.parentElement.children) {
        if (el instanceof HTMLElement && el !== branch) {
          siblings.push({ el, inert: el.inert });
          el.inert = true;
        }
      }
      branch = branch.parentElement;
    }
    return () => {
      for (const { el, inert } of siblings) el.inert = inert;
      setActivator?.(null);
    };
  }, [active, setActivator]);
  if (layoutOnly && !layout) return null;
  return (
    <div
      ref={ref}
      data-pane-title-row
      className={cn("flex min-h-8 items-center gap-2", className, active && "live-pane-title-edit")}
      role={active ? "group" : undefined}
      onPointerDown={
        active
          ? (e) => {
              if (e.target instanceof Element && e.target.closest("[data-layout-controls]")) return;
              layout.drag.listeners?.onPointerDown?.(e);
            }
          : undefined
      }
    >
      {active && (
        <button
          type="button"
          ref={grip}
          data-layout-grip
          className="live-pane-grip flex size-8 shrink-0 items-center justify-center rounded-lg"
          {...layout.drag.attributes}
          aria-label={t`Drag ${layout.title}`}
        >
          <GripVertical aria-hidden="true" className="size-4" />
        </button>
      )}
      {children}
      {active && (
        <span data-layout-controls className="ml-auto flex shrink-0 gap-1">
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={t`Add beside ${layout.title}`}
            disabled={layout.full}
            onClick={layout.beside}
          >
            <Plus />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={t`Hide ${layout.title}`}
            onClick={layout.hide}
          >
            <X />
          </Button>
        </span>
      )}
    </div>
  );
}
