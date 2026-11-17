import { t } from "@lingui/core/macro";
import { type PointerEvent as ReactPointerEvent, type RefObject, useEffect, useRef } from "react";
import { type Action, minimum, type Row, resizePair, type VisualSlot } from "./pane-layout-model";

function useResize() {
  const stop = useRef<(() => void) | null>(null);
  useEffect(() => () => stop.current?.(), []);
  return (
    event: ReactPointerEvent,
    preview: (x: number, y: number) => void,
    commit: () => void,
    restore: () => void,
  ) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    stop.current?.();
    const target = event.currentTarget;
    const pointerId = event.pointerId;
    target.setPointerCapture(pointerId);
    let frame = 0;
    let x = event.clientX;
    let y = event.clientY;
    const draw = () => {
      frame = 0;
      preview(x, y);
    };
    const move = (e: PointerEvent) => {
      x = e.clientX;
      y = e.clientY;
      if (!frame) frame = requestAnimationFrame(draw);
    };
    const cleanup = () => {
      cancelAnimationFrame(frame);
      if (target.hasPointerCapture(pointerId)) target.releasePointerCapture(pointerId);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("keydown", key);
      stop.current = null;
    };
    const cancel = () => {
      cleanup();
      restore();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") cancel();
    };
    const end = (e: PointerEvent) => {
      x = e.clientX;
      y = e.clientY;
      cleanup();
      preview(x, y);
      commit();
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("keydown", key);
    stop.current = cancel;
  };
}

export function SlotDivider({
  container,
  row,
  source,
  left,
  right,
  edit,
}: {
  container: RefObject<HTMLDivElement | null>;
  row: Row;
  source: number;
  left: VisualSlot;
  right: VisualSlot;
  edit: (action: Action) => void;
}) {
  const start = useResize();
  const elements = () =>
    [left, right].map((s) =>
      container.current?.querySelector<HTMLElement>(`[data-slot="${s.index}"]`),
    );
  const values = () =>
    elements().map((el) => el?.getBoundingClientRect().width ?? 0) as [number, number];
  const pixelsPerWeight = () =>
    Math.min(
      ...Array.from(
        container.current?.querySelectorAll<HTMLElement>(":scope > .live-slot") ?? [],
        (el) => el.getBoundingClientRect().width / (row.slots[Number(el.dataset.slot)]?.size ?? 1),
      ),
    );
  const sizes: [number, number] = [
    row.slots[left.index]?.size ?? left.slot.size,
    row.slots[right.index]?.size ?? right.slot.size,
  ];
  const mins: [number, number] = [minimum(left.slot), minimum(right.slot)];
  const commit = (next: [number, number]) =>
    edit({ type: "sizes", row: source, left: left.index, right: right.index, sizes: next });
  const equalize = () => {
    const pixels = values();
    commit(resizePair(sizes, pixels, (pixels[1] - pixels[0]) / 2, mins, pixelsPerWeight()));
  };
  return (
    <button
      type="button"
      className="live-slot-divider"
      aria-label={t`Resize columns ${left.index + 1} and ${right.index + 1}`}
      title={t`Drag to resize · double-click or Enter to equalize`}
      onDoubleClick={equalize}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          equalize();
        }
        if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
          e.preventDefault();
          const pixels = values();
          commit(
            resizePair(
              sizes,
              pixels,
              (pixels[0] + pixels[1]) * (e.key === "ArrowLeft" ? -0.05 : 0.05),
              mins,
              pixelsPerWeight(),
            ),
          );
        }
      }}
      onPointerDown={(e) => {
        const els = elements();
        const pixels = values();
        const origin = e.clientX;
        const unit = pixelsPerWeight();
        let next = sizes;
        const restore = () =>
          els.forEach((el, i) => {
            el?.style.setProperty("--slot-width", `${pixels[i]}px`);
          });
        start(
          e,
          (x) => {
            next = resizePair(sizes, pixels, x - origin, mins, unit);
            const scale = Math.max(...next);
            const sum = next[0] / scale + next[1] / scale;
            const total = pixels[0] + pixels[1];
            els.forEach((el, i) => {
              if (next !== sizes)
                el?.style.setProperty(
                  "--slot-width",
                  `${total * ((next[i] ?? 0) / scale / sum)}px`,
                );
            });
          },
          () => commit(next),
          restore,
        );
      }}
    />
  );
}

export function RowDivider({
  container,
  source,
  minHeight,
  edit,
}: {
  container: RefObject<HTMLDivElement | null>;
  source: number;
  minHeight?: number;
  edit: (action: Action) => void;
}) {
  const start = useResize();
  return (
    <button
      type="button"
      className="live-row-divider"
      aria-label={t`Resize row ${source + 1} height`}
      title={t`Drag to set minimum height · double-click for auto`}
      onDoubleClick={() => edit({ type: "height", row: source })}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === "Home") {
          e.preventDefault();
          edit({ type: "height", row: source });
        }
        if (e.key === "ArrowUp" || e.key === "ArrowDown") {
          e.preventDefault();
          edit({
            type: "height",
            row: source,
            value: Math.max(
              80,
              (minHeight ?? container.current?.getBoundingClientRect().height ?? 80) +
                (e.key === "ArrowUp" ? -20 : 20),
            ),
          });
        }
      }}
      onPointerDown={(e) => {
        const el = container.current;
        if (!el) return;
        const origin = e.clientY;
        const height = el.getBoundingClientRect().height;
        let next = height;
        start(
          e,
          (_, y) => {
            next = Math.max(80, height + y - origin);
            el.style.setProperty("--row-min-height", `${next}px`);
          },
          () => edit({ type: "height", row: source, value: next }),
          () => el.style.setProperty("--row-min-height", `${minHeight ?? 0}px`),
        );
      }}
    />
  );
}
