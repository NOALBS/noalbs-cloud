import type { FeedState, Marker, StreamSummary } from "@/api/types";
import { currentLocale } from "@/lib/locale";

/** "4h 12m", "12m 5s", "45s". */
export function formatDuration(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${s % 60}s`;
  return `${s}s`;
}

export interface StateSeconds {
  live_seconds: number;
  low_seconds: number;
  offline_seconds: number;
}

/** Each state's share of the time, 0–1 (all 0 when nothing was recorded). */
export function stateShares(seconds: StateSeconds): { live: number; low: number; offline: number } {
  const total = seconds.live_seconds + seconds.low_seconds + seconds.offline_seconds;
  if (total === 0) return { live: 0, low: 0, offline: 0 };
  return {
    live: seconds.live_seconds / total,
    low: seconds.low_seconds / total,
    offline: seconds.offline_seconds / total,
  };
}

/** The worst state in a step, so a one-second drop still shows. */
export function worstState(seconds: StateSeconds): FeedState {
  if (seconds.offline_seconds > 0) return "offline";
  if (seconds.low_seconds > 0) return "low";
  if (seconds.live_seconds > 0) return "live";
  return "idle";
}

/** Streams grouped by the local day they started, keeping their order. */
export function groupByDay<T extends Pick<StreamSummary, "started_at">>(
  streams: T[],
  locale = currentLocale(),
): { key: string; label: string; streams: T[] }[] {
  const groups: { key: string; label: string; streams: T[] }[] = [];
  for (const stream of streams) {
    const date = new Date(stream.started_at);
    const key = `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`;
    let group = groups[groups.length - 1];
    if (group?.key !== key) {
      group = {
        key,
        label: date.toLocaleDateString(locale, {
          weekday: "long",
          day: "numeric",
          month: "long",
        }),
        streams: [],
      };
      groups.push(group);
    }
    group.streams.push(stream);
  }
  return groups;
}

/**
 * Every number in a JSON value by its path, e.g. `publishers.live.bitrate`.
 * Array items use their index. Numeric strings don't count.
 */
export function flattenNumbers(value: unknown, prefix = "", out: Record<string, number> = {}) {
  if (typeof value === "number" && Number.isFinite(value)) {
    if (prefix) out[prefix] = value;
  } else if (Array.isArray(value)) {
    value.forEach((item, index) => {
      flattenNumbers(item, prefix ? `${prefix}.${index}` : String(index), out);
    });
  } else if (value && typeof value === "object") {
    for (const [key, item] of Object.entries(value)) {
      flattenNumbers(item, prefix ? `${prefix}.${key}` : key, out);
    }
  }
  return out;
}

/** The numeric fields found in any of the snapshots, sorted. */
export function numericFields(snapshots: unknown[]): string[] {
  const fields = new Set<string>();
  for (const snapshot of snapshots) {
    for (const field of Object.keys(flattenNumbers(snapshot))) fields.add(field);
  }
  return [...fields].sort();
}

/** The point whose step contains `at`, if any (points sorted by time). */
export function pointAt<T extends { at: number }>(
  points: T[],
  at: number,
  stepMs: number,
): T | undefined {
  let low = 0;
  let high = points.length - 1;
  while (low <= high) {
    const mid = (low + high) >> 1;
    const point = points[mid];
    if (!point) break;
    if (at < point.at) high = mid - 1;
    else if (at >= point.at + stepMs) low = mid + 1;
    else return point;
  }
  return undefined;
}

/** The point nearest to `at` (points sorted by time). */
export function nearest<T extends { at: number }>(points: T[], at: number): T | undefined {
  let best: T | undefined;
  for (const point of points) {
    if (!best || Math.abs(point.at - at) < Math.abs(best.at - at)) best = point;
  }
  return best;
}

export interface ChartPoint {
  at: number;
  value: number | null;
  min?: number | null;
  max?: number | null;
}

/**
 * Runs of points without gaps: a step with no data (the browser source was
 * away) breaks the line instead of drawing across it.
 */
export function segments<T extends { at: number }>(points: T[], stepMs: number): T[][] {
  const runs: T[][] = [];
  let run: T[] = [];
  for (const point of points) {
    const previous = run[run.length - 1];
    if (previous && point.at - previous.at > stepMs * 1.5) {
      runs.push(run);
      run = [];
    }
    run.push(point);
  }
  if (run.length > 0) runs.push(run);
  return runs;
}

/** An SVG path for the line, one subpath per run of values. */
export function linePath(
  points: ChartPoint[],
  stepMs: number,
  x: (at: number) => number,
  y: (value: number) => number,
): string {
  return segments(
    points.filter((point) => point.value != null),
    stepMs,
  )
    .map((run) =>
      run
        .map(
          (point, index) =>
            `${index === 0 ? "M" : "L"}${x(point.at + stepMs / 2).toFixed(1)},${y(point.value ?? 0).toFixed(1)}`,
        )
        .join(""),
    )
    .join("");
}

/** An SVG path for the min–max band, one closed shape per run. */
export function bandPath(
  points: ChartPoint[],
  stepMs: number,
  x: (at: number) => number,
  y: (value: number) => number,
): string {
  return segments(
    points.filter((point) => point.min != null && point.max != null),
    stepMs,
  )
    .map((run) => {
      const top = run.map((p) => `${x(p.at + stepMs / 2).toFixed(1)},${y(p.max ?? 0).toFixed(1)}`);
      const bottom = run
        .map((p) => `${x(p.at + stepMs / 2).toFixed(1)},${y(p.min ?? 0).toFixed(1)}`)
        .reverse();
      return `M${[...top, ...bottom].join("L")}Z`;
    })
    .join("");
}

/** A round number just above `value`, for the top of an axis. */
export function niceCeil(value: number): number {
  if (value <= 0) return 1;
  const magnitude = 10 ** Math.floor(Math.log10(value));
  for (const step of [1, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10]) {
    if (step * magnitude >= value) return step * magnitude;
  }
  return 10 * magnitude;
}

const TICK_STEPS_S = [10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200, 10800, 21600, 43200];

/** Evenly spaced round times between `from` and `to`, at most `max` of them. */
export function timeTicks(from: number, to: number, max = 6): number[] {
  const span = to - from;
  const step = (TICK_STEPS_S.find((seconds) => span / (seconds * 1000) <= max) ?? 86_400) * 1000;
  // Round in local time, so hourly ticks land on the hour where you are.
  const offset = new Date(from).getTimezoneOffset() * 60_000;
  const first = Math.ceil((from - offset) / step) * step + offset;
  const ticks: number[] = [];
  for (let at = first; at <= to; at += step) ticks.push(at);
  return ticks;
}

export function formatClock(at: number, seconds = false): string {
  return new Date(at).toLocaleTimeString(currentLocale(), {
    hourCycle: "h23",
    hour: "2-digit",
    minute: "2-digit",
    ...(seconds ? { second: "2-digit" } : {}),
  });
}

/** A time into a stream: "4:07", or "1:02:05" from an hour on (what chapter lists use). */
export function formatOffset(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s % 60)}` : `${m}:${pad(s % 60)}`;
}

/** One line per marker, "1:02:05 Clutch play", for pasting or saving. */
export function markersToText(markers: Marker[]): string {
  return markers
    .map((marker) => `${formatOffset(marker.offset_seconds)} ${marker.description ?? "Marker"}`)
    .join("\n");
}
