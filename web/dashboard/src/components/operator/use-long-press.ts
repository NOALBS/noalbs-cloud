import { type MouseEvent, type PointerEvent, useCallback, useEffect, useRef } from "react";

export function useLongPress(onOpen: () => void) {
  const callback = useRef(onOpen);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const origin = useRef<{ x: number; y: number } | null>(null);
  const held = useRef(false);
  useEffect(() => {
    callback.current = onOpen;
  }, [onOpen]);
  const cancel = useCallback(() => {
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = null;
    origin.current = null;
  }, []);
  useEffect(() => {
    window.addEventListener("scroll", cancel, true);
    return () => {
      cancel();
      window.removeEventListener("scroll", cancel, true);
    };
  }, [cancel]);
  return {
    onPointerDown: (event: PointerEvent<HTMLElement>) => {
      cancel();
      held.current = false;
      if (
        event.button !== 0 ||
        event.isPrimary === false ||
        (event.target as Element).closest("button")
      )
        return;
      origin.current = { x: event.clientX, y: event.clientY };
      timer.current = setTimeout(() => {
        cancel();
        held.current = true;
        callback.current();
      }, 450);
    },
    onPointerMove: (event: PointerEvent<HTMLElement>) => {
      if (
        origin.current &&
        Math.hypot(event.clientX - origin.current.x, event.clientY - origin.current.y) > 8
      )
        cancel();
    },
    onPointerUp: cancel,
    onPointerCancel: cancel,
    onPointerLeave: cancel,
    onClickCapture: (event: MouseEvent<HTMLElement>) => {
      if (held.current) {
        held.current = false;
        if (event.detail === 0) return;
        event.preventDefault();
        event.stopPropagation();
      }
    },
    onContextMenu: (event: MouseEvent<HTMLElement>) => {
      if ((event.target as Element).closest("button")) return;
      event.preventDefault();
      cancel();
      held.current = true;
      callback.current();
    },
  };
}
