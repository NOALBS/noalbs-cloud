import { type PointerEvent, useRef, useState } from "react";
import type { FeedState } from "@/api/types";
import { DESKTOP, useMediaQuery } from "@/hooks/use-media-query";
import {
  bandPath,
  type ChartPoint,
  formatClock,
  linePath,
  niceCeil,
  timeTicks,
} from "@/lib/history";
import { currentLocale } from "@/lib/locale";
import { cn } from "@/lib/utils";

const WIDTH = 1000;
const PAD = 6;

export interface ChartLine {
  points: ChartPoint[];
  /** Text colour class; the line and band draw with currentColor. */
  tone: string;
  /** Draw the min–max band behind the line. */
  band?: boolean;
}

export interface Threshold {
  value: number;
  /** Stroke colour class, e.g. `stroke-low`. */
  className: string;
}

export interface Marker {
  at: number;
  highlighted?: boolean;
}

/** Where `event` is along the chart, as a time. */
function timeAt(event: PointerEvent<HTMLDivElement>, from: number, to: number): number {
  const rect = event.currentTarget.getBoundingClientRect();
  const fraction = Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width));
  return from + fraction * (to - from);
}

const percent = (at: number, from: number, to: number) => `${((at - from) / (to - from)) * 100}%`;

/**
 * A time chart over a whole stream (or a zoomed part): average line, optional
 * min–max band, dashed trigger lines and event markers. Hovering reports the time
 * under the pointer so every chart on the page can show the same moment; dragging
 * across selects a range to zoom into. Pure SVG, like the live chart.
 */
export function HistoryChart({
  from,
  to,
  stepMs,
  lines,
  thresholds = [],
  markers = [],
  minMax = 1,
  hoverAt,
  onHover,
  onZoom,
  formatValue = (value) => value.toLocaleString(currentLocale()),
  height = 160,
  className,
}: {
  from: number;
  to: number;
  stepMs: number;
  lines: ChartLine[];
  thresholds?: Threshold[];
  markers?: Marker[];
  /** The top of the axis is at least this. */
  minMax?: number;
  hoverAt: number | null;
  onHover: (at: number | null) => void;
  onZoom?: (from: number, to: number) => void;
  formatValue?: (value: number) => string;
  height?: number;
  className?: string;
}) {
  const [drag, setDrag] = useState<{ start: number; end: number } | null>(null);
  const dragging = useRef(false);

  const highest = Math.max(
    minMax,
    ...lines.flatMap((line) =>
      line.points.map((point) => (line.band ? (point.max ?? point.value) : point.value) ?? 0),
    ),
    ...thresholds.map((threshold) => threshold.value),
  );
  const yMax = niceCeil(highest * 1.05);
  const x = (at: number) => ((at - from) / (to - from)) * WIDTH;
  const y = (value: number) => height - PAD - (Math.min(value, yMax) / yMax) * (height - PAD * 2);

  const down = (event: PointerEvent<HTMLDivElement>) => {
    if (event.pointerType === "mouse" && event.button !== 0) return;
    const at = timeAt(event, from, to);
    event.currentTarget.setPointerCapture(event.pointerId);
    dragging.current = true;
    setDrag({ start: at, end: at });
    onHover(at);
  };
  const move = (event: PointerEvent<HTMLDivElement>) => {
    const at = timeAt(event, from, to);
    onHover(at);
    if (dragging.current) setDrag((current) => current && { ...current, end: at });
  };
  const up = () => {
    dragging.current = false;
    if (drag && onZoom && Math.abs(drag.end - drag.start) > (to - from) * 0.02) {
      onZoom(Math.min(drag.start, drag.end), Math.max(drag.start, drag.end));
    }
    setDrag(null);
  };

  return (
    <div className={cn("relative select-none", className)} style={{ height }}>
      <svg
        viewBox={`0 0 ${WIDTH} ${height}`}
        preserveAspectRatio="none"
        className="absolute inset-0 size-full overflow-hidden"
        aria-hidden="true"
      >
        {[yMax, yMax / 2].map((value) => (
          <line
            key={value}
            x1="0"
            x2={WIDTH}
            y1={y(value)}
            y2={y(value)}
            className="stroke-divider"
            strokeWidth="1"
            vectorEffect="non-scaling-stroke"
          />
        ))}
        <line
          x1="0"
          x2={WIDTH}
          y1={y(0)}
          y2={y(0)}
          className="stroke-border"
          strokeWidth="1"
          vectorEffect="non-scaling-stroke"
        />
        {markers.map((marker) => (
          <line
            key={`${marker.at}-${marker.highlighted}`}
            x1={x(marker.at)}
            x2={x(marker.at)}
            y1="0"
            y2={height}
            className={marker.highlighted ? "stroke-primary" : "stroke-foreground/15"}
            strokeWidth={marker.highlighted ? 2 : 1}
            vectorEffect="non-scaling-stroke"
          />
        ))}
        {thresholds.map((threshold) => (
          <line
            key={`${threshold.className}-${threshold.value}`}
            x1="0"
            x2={WIDTH}
            y1={y(threshold.value)}
            y2={y(threshold.value)}
            className={threshold.className}
            strokeDasharray="4 5"
            strokeWidth="1"
            vectorEffect="non-scaling-stroke"
          />
        ))}
        {lines.map((line, index) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: lines are fixed per chart
          <g key={index} className={line.tone}>
            {line.band && (
              <path d={bandPath(line.points, stepMs, x, y)} className="fill-current opacity-15" />
            )}
            <path
              d={linePath(line.points, stepMs, x, y)}
              className="fill-none stroke-current"
              strokeWidth="1.75"
              strokeLinejoin="round"
              strokeLinecap="round"
              vectorEffect="non-scaling-stroke"
            />
          </g>
        ))}
      </svg>

      <span className="pointer-events-none absolute top-0 left-1 font-mono text-[10px] text-muted-foreground tabular">
        {formatValue(yMax)}
      </span>
      <span
        className="pointer-events-none absolute left-1 -translate-y-full font-mono text-[10px] text-muted-foreground tabular"
        style={{ top: y(yMax / 2) }}
      >
        {formatValue(yMax / 2)}
      </span>

      {drag && drag.start !== drag.end && (
        <div
          className="pointer-events-none absolute inset-y-0 bg-primary/15"
          style={{
            left: percent(Math.min(drag.start, drag.end), from, to),
            width: percent(from + Math.abs(drag.end - drag.start), from, to),
          }}
        />
      )}
      {hoverAt != null && hoverAt >= from && hoverAt <= to && (
        <div
          className="pointer-events-none absolute inset-y-0 w-px bg-foreground/50"
          style={{ left: percent(hoverAt, from, to) }}
        />
      )}

      <div
        className="absolute inset-0 cursor-crosshair touch-pan-y"
        onPointerDown={down}
        onPointerMove={move}
        onPointerUp={up}
        onPointerCancel={() => {
          dragging.current = false;
          setDrag(null);
        }}
        onPointerLeave={(event) => {
          if (event.pointerType === "mouse" && !dragging.current) onHover(null);
        }}
      />
    </div>
  );
}

const STATE_FILL: Record<FeedState, string> = {
  live: "fill-live",
  low: "fill-low",
  offline: "fill-offline",
  idle: "fill-idle",
};

/** A thin bar coloured by the state in each step: where the feed struggled. */
export function StateStrip({
  from,
  to,
  stepMs,
  points,
  className,
}: {
  from: number;
  to: number;
  stepMs: number;
  points: { at: number; state: FeedState }[];
  className?: string;
}) {
  const x = (at: number) => ((at - from) / (to - from)) * WIDTH;
  return (
    <svg
      viewBox={`0 0 ${WIDTH} 1`}
      preserveAspectRatio="none"
      className={cn("h-1.5 w-full overflow-hidden rounded-full bg-muted", className)}
      aria-hidden="true"
    >
      {points.map((point) => (
        <rect
          key={point.at}
          x={x(point.at)}
          width={Math.max(0.5, x(point.at + stepMs) - x(point.at))}
          y="0"
          height="1"
          className={STATE_FILL[point.state]}
        />
      ))}
    </svg>
  );
}

/** Clock times under a chart. */
export function TimeAxis({
  from,
  to,
  className,
}: {
  from: number;
  to: number;
  className?: string;
}) {
  const desktop = useMediaQuery(DESKTOP);
  const ticks = timeTicks(from, to, desktop ? 6 : 4);
  // Seconds only when ticks are closer than a minute, e.g. zoomed in.
  const seconds = ticks.length > 1 && (ticks[1] ?? 0) - (ticks[0] ?? 0) < 60_000;
  return (
    <div className={cn("relative h-4", className)} aria-hidden="true">
      {ticks.map((at) => {
        const position = (at - from) / (to - from);
        return (
          <span
            key={at}
            className={cn(
              "absolute font-mono text-[10px] text-muted-foreground tabular",
              // Keep labels at the edges inside the chart.
              position < 0.05 ? "" : position > 0.95 ? "-translate-x-full" : "-translate-x-1/2",
            )}
            style={{ left: percent(at, from, to) }}
          >
            {formatClock(at, seconds)}
          </span>
        );
      })}
    </div>
  );
}
