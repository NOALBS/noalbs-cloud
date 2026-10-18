import { cn } from "@/lib/utils";
import { chartEnd, HISTORY_MARGIN_SECONDS, HISTORY_SECONDS, type Point } from "@/live/reducer";

const WIDTH = 760;

/**
 * Bitrate over the last two minutes, with the LOW and BRB triggers as dashed
 * lines (from the mockup). Pure SVG: no chart library needed for one line.
 */
export function BitrateChart({
  points,
  now,
  low,
  offline,
  tone,
  height = 150,
  thresholds = true,
  className,
}: {
  points: Point[];
  now: number;
  low: number | null | undefined;
  offline: number | null | undefined;
  /** Text colour class for the line, e.g. `text-live` (drawn with currentColor). */
  tone: string;
  height?: number;
  thresholds?: boolean;
  className?: string;
}) {
  const pad = 6;
  const max = Math.max(8000, ...points.map((point) => point.bitrate * 1.1));
  const start = chartEnd(points, now) - HISTORY_SECONDS * 1000;
  const x = (at: number) => ((at - start) / (HISTORY_SECONDS * 1000)) * WIDTH;
  const y = (bitrate: number) => height - pad - (Math.min(bitrate, max) / max) * (height - pad * 2);

  const line = points
    // Points just past the left edge keep the line running to it; the SVG clips them.
    .filter((point) => point.at >= start - HISTORY_MARGIN_SECONDS * 1000)
    .map((point) => `${x(point.at).toFixed(1)},${y(point.bitrate).toFixed(1)}`);
  const first = line[0]?.split(",")[0];
  const last = line[line.length - 1]?.split(",")[0];

  return (
    <svg
      viewBox={`0 0 ${WIDTH} ${height}`}
      preserveAspectRatio="none"
      className={cn("w-full overflow-hidden", className)}
      style={{ height }}
      aria-hidden="true"
    >
      {thresholds && low != null && (
        <line
          x1="0"
          x2={WIDTH}
          y1={y(low)}
          y2={y(low)}
          className="stroke-low"
          strokeDasharray="4 5"
          strokeWidth="1"
          vectorEffect="non-scaling-stroke"
        />
      )}
      {thresholds && offline != null && (
        <line
          x1="0"
          x2={WIDTH}
          y1={y(offline)}
          y2={y(offline)}
          className="stroke-offline"
          strokeDasharray="4 5"
          strokeWidth="1"
          vectorEffect="non-scaling-stroke"
        />
      )}
      {line.length > 1 && (
        <>
          <polygon
            points={`${first},${height} ${line.join(" ")} ${last},${height}`}
            className={cn("fill-current opacity-10", tone)}
          />
          <polyline
            points={line.join(" ")}
            className={cn("fill-none stroke-current", tone)}
            strokeWidth="2"
            strokeLinejoin="round"
            strokeLinecap="round"
            vectorEffect="non-scaling-stroke"
          />
        </>
      )}
    </svg>
  );
}
