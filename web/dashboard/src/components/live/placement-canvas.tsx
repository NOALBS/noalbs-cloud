import { t } from "@lingui/core/macro";
import { type KeyboardEvent, type PointerEvent, useRef, useState } from "react";
import { type Box, type Canvas, type Corner, moved, resized } from "@/lib/placement";
import { cn } from "@/lib/utils";

interface Drag {
  kind: "move" | Corner;
  start: Box;
  pointerX: number;
  pointerY: number;
  /** Screen pixels per canvas pixel. */
  scale: number;
}

const CORNERS: { corner: Corner; className: string; cursor: string }[] = [
  { corner: "top_left", className: "left-0 top-0", cursor: "cursor-nwse-resize" },
  { corner: "top_right", className: "left-full top-0", cursor: "cursor-nesw-resize" },
  { corner: "bottom_left", className: "left-0 top-full", cursor: "cursor-nesw-resize" },
  { corner: "bottom_right", className: "left-full top-full", cursor: "cursor-nwse-resize" },
];

/** CSS for a box on the canvas, in percent so it scales with the drawing. */
function at(box: Box, canvas: Canvas) {
  return {
    left: `${(box.x / canvas.width) * 100}%`,
    top: `${(box.y / canvas.height) * 100}%`,
    width: `${(box.width / canvas.width) * 100}%`,
    height: `${(box.height / canvas.height) * 100}%`,
  };
}

/**
 * The OBS canvas, drawn to fit: the source being placed as a box to drag
 * (move it, or pull a corner to resize it), the scene's other sources as
 * outlines, and the preview behind, if there is one. Only the box and its
 * corners take touches, so the page still scrolls over the rest.
 */
export function PlacementCanvas({
  canvas,
  box,
  content,
  onChange,
  name,
  others,
  background,
  keepShape = true,
}: {
  canvas: Canvas;
  /** The box the source is sized to. */
  box: Box;
  /** Where the source shows in it, if not all of it (a tall chat fit to a wide box). */
  content?: Box | null;
  /** Without it, the box is only drawn: for showing what a preset or numbers do. */
  onChange?: (box: Box) => void;
  name: string;
  /** The scene's other sources, top first; hidden ones are drawn fainter. */
  others: { key: string; name: string; enabled: boolean; box: Box }[];
  /** A still of the scene, drawn under everything. */
  background: string | null;
  keepShape?: boolean;
}) {
  const surface = useRef<HTMLDivElement>(null);
  const drag = useRef<Drag | null>(null);
  const [guides, setGuides] = useState<{ x: number[]; y: number[] }>({ x: [], y: [] });

  const begin = (kind: Drag["kind"]) => (event: PointerEvent<HTMLElement>) => {
    const rect = surface.current?.getBoundingClientRect();
    if (!rect || rect.width === 0) return;
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = {
      kind,
      start: box,
      pointerX: event.clientX,
      pointerY: event.clientY,
      scale: rect.width / canvas.width,
    };
  };

  const follow = (event: PointerEvent<HTMLElement>) => {
    const current = drag.current;
    if (!current) return;
    const dx = (event.clientX - current.pointerX) / current.scale;
    const dy = (event.clientY - current.pointerY) / current.scale;
    if (current.kind === "move") {
      const next = moved(current.start, dx, dy, canvas);
      setGuides(next.guides);
      onChange?.(next.box);
    } else {
      onChange?.(resized(current.start, current.kind, dx, dy, keepShape, canvas));
    }
  };

  const end = () => {
    drag.current = null;
    setGuides({ x: [], y: [] });
  };

  // Arrow keys move it a pixel (ten with Shift), for keyboards and fine tuning.
  const nudge = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = event.shiftKey ? 10 : 1;
    const by = {
      ArrowLeft: [-step, 0],
      ArrowRight: [step, 0],
      ArrowUp: [0, -step],
      ArrowDown: [0, step],
    }[event.key];
    if (!by) return;
    event.preventDefault();
    onChange?.({ ...box, x: box.x + (by[0] ?? 0), y: box.y + (by[1] ?? 0) });
  };

  return (
    // Room around the screen, so corners on its edge are whole and easy to grab.
    <div className="-mx-1 shrink-0 overflow-hidden p-3.5 select-none">
      <div
        ref={surface}
        className="relative w-full"
        style={{ aspectRatio: `${canvas.width} / ${canvas.height}` }}
      >
        {/* The picture, cut to the screen; the box goes over it, uncut. */}
        <div className="absolute inset-0 overflow-hidden rounded-lg bg-black">
          {background && (
            <img
              src={background}
              alt=""
              className="pointer-events-none absolute inset-0 size-full object-fill opacity-60"
            />
          )}
          {others.map((other) => (
            <div
              key={other.key}
              className={cn(
                "pointer-events-none absolute overflow-hidden border border-dashed px-1 text-[10px] leading-4",
                other.enabled ? "border-white/40 text-white/60" : "border-white/15 text-white/25",
              )}
              style={at(other.box, canvas)}
            >
              {/* Full-screen ones all share the screen's edge: their names would pile up. */}
              {!(
                other.box.width >= canvas.width * 0.9 && other.box.height >= canvas.height * 0.9
              ) && <span className="block truncate">{other.name}</span>}
            </div>
          ))}
          {guides.x.map((x) => (
            <div
              key={`x${x}`}
              className="pointer-events-none absolute inset-y-0 w-px bg-primary"
              style={{ left: `${(x / canvas.width) * 100}%` }}
            />
          ))}
          {guides.y.map((y) => (
            <div
              key={`y${y}`}
              className="pointer-events-none absolute inset-x-0 h-px bg-primary"
              style={{ top: `${(y / canvas.height) * 100}%` }}
            />
          ))}
        </div>
        {/* Where the source itself shows, when it doesn't fill its box. */}
        {content && (
          <div className="pointer-events-none absolute bg-primary/35" style={at(content, canvas)} />
        )}
        {onChange ? (
          <div
            role="slider"
            tabIndex={0}
            aria-label={t`${name}: drag to move, pull a corner to resize, or use the arrow keys`}
            aria-valuetext={t`${Math.round(box.x)}, ${Math.round(box.y)}, ${Math.round(box.width)} by ${Math.round(box.height)}`}
            aria-valuenow={Math.round(box.x)}
            onPointerDown={begin("move")}
            onPointerMove={follow}
            onPointerUp={end}
            onPointerCancel={end}
            onKeyDown={nudge}
            className={cn(
              "absolute cursor-move touch-none border-2 border-primary outline-none focus-visible:ring-2 focus-visible:ring-white",
              !content && "bg-primary/20",
            )}
            style={at(box, canvas)}
          >
            <BoxLabel name={name} />
            {CORNERS.map(({ corner, className, cursor }) => (
              <span
                key={corner}
                aria-hidden="true"
                onPointerDown={begin(corner)}
                onPointerMove={follow}
                onPointerUp={end}
                onPointerCancel={end}
                // A finger-sized target around a small dot.
                className={cn(
                  "absolute flex size-7 -translate-x-1/2 -translate-y-1/2 touch-none items-center justify-center",
                  className,
                  cursor,
                )}
              >
                <span className="size-3 rounded-full border-2 border-white bg-primary" />
              </span>
            ))}
          </div>
        ) : (
          <div
            role="img"
            aria-label={t`Where ${name} would be: ${Math.round(box.x)}, ${Math.round(box.y)}, ${Math.round(box.width)} by ${Math.round(box.height)}`}
            className={cn(
              "pointer-events-none absolute border-2 border-dashed border-primary",
              !content && "bg-primary/20",
            )}
            style={at(box, canvas)}
          >
            <BoxLabel name={name} />
          </div>
        )}
      </div>
    </div>
  );
}

function BoxLabel({ name }: { name: string }) {
  return (
    <span className="pointer-events-none absolute top-0 left-0 max-w-full truncate bg-primary px-1 text-[10px] leading-4 font-semibold text-primary-foreground">
      {name}
    </span>
  );
}
