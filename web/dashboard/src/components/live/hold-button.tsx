import { type ComponentProps, useCallback, useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

export const HOLD_MS = 1000;

export function HoldButton({
  onComplete,
  children,
  disabled,
  className,
  ...props
}: Omit<ComponentProps<"button">, "onClick"> & { onComplete: () => void }) {
  const [holding, setHolding] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const active = useRef(false);
  const action = useRef(onComplete);
  const unavailable = useRef(disabled);
  action.current = onComplete;
  unavailable.current = disabled;
  const cancel = useCallback(() => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = null;
    active.current = false;
    setHolding(false);
  }, []);
  const start = () => {
    if (unavailable.current || active.current) return;
    active.current = true;
    setHolding(true);
    timer.current = setTimeout(() => {
      timer.current = null;
      setHolding(false);
      if (!unavailable.current) action.current();
    }, HOLD_MS);
  };
  useEffect(() => {
    if (disabled) cancel();
  }, [disabled, cancel]);
  useEffect(() => {
    const cancelHidden = () => {
      if (document.hidden) cancel();
    };
    window.addEventListener("blur", cancel);
    document.addEventListener("visibilitychange", cancelHidden);
    return () => {
      if (timer.current) clearTimeout(timer.current);
      window.removeEventListener("blur", cancel);
      document.removeEventListener("visibilitychange", cancelHidden);
    };
  }, [cancel]);
  return (
    <Button
      variant="outline"
      {...props}
      type="button"
      disabled={disabled}
      className={cn("hold-button", className)}
      data-holding={holding}
      data-live-hold
      onPointerDown={(event) => {
        if (event.button === 0) {
          event.currentTarget.setPointerCapture?.(event.pointerId);
          start();
        }
      }}
      onPointerUp={cancel}
      onPointerMove={(event) => {
        if (!active.current) return;
        const bounds = event.currentTarget.getBoundingClientRect();
        if (
          event.clientX < bounds.left ||
          event.clientX > bounds.right ||
          event.clientY < bounds.top ||
          event.clientY > bounds.bottom
        )
          cancel();
      }}
      onPointerCancel={cancel}
      onPointerLeave={cancel}
      onLostPointerCapture={cancel}
      onBlur={cancel}
      onKeyDown={(event) => {
        if (event.key === " " || event.key === "Enter") {
          event.preventDefault();
          if (!event.repeat) start();
        }
      }}
      onKeyUp={(event) => {
        if (event.key === " " || event.key === "Enter") {
          event.preventDefault();
          cancel();
        }
      }}
      onClick={(event) => event.preventDefault()}
    >
      <span aria-hidden className="hold-fill" />
      <span className="hold-label">{children}</span>
      <span className="sr-only">Hold for one second to confirm</span>
    </Button>
  );
}
