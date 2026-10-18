import type { ObsDropped, ObsStats } from "@/api/types";
import { currentLocale } from "@/lib/locale";

/** How often the OBS stats popover asks OBS (each costs a round trip to the streamer's OBS). */
export const OBS_STATS_INTERVAL = 2_000;

/** Readings further apart than this (a cached one from earlier) don't make a bitrate. */
const MAX_BITRATE_GAP_MS = 10_000;

/**
 * The stream's bitrate in kbps between two readings of the same running stream,
 * like NOALBS's `!serverinfo`; `null` otherwise.
 */
export function bitrateBetween(earlier: ObsStats, later: ObsStats): number | null {
  if (!earlier.streaming || !later.streaming) return null;
  const bytes = later.stream_bytes - earlier.stream_bytes;
  const ms = later.stream_duration_ms - earlier.stream_duration_ms;
  if (bytes < 0 || ms <= 0 || ms > MAX_BITRATE_GAP_MS) return null;
  return Math.round((bytes * 8) / ms);
}

export function droppedPercent(dropped: ObsDropped): number {
  return dropped.total === 0 ? 0 : (dropped.frames / dropped.total) * 100;
}

/** "12 (2.0%)", like OBS's Stats dock. */
export function formatDropped(dropped: ObsDropped): string {
  return `${dropped.frames.toLocaleString(currentLocale())} (${droppedPercent(dropped).toFixed(1)}%)`;
}

/** None dropped, a few (under 1%), or enough to see on stream. */
export function droppedTone(dropped: ObsDropped): "ok" | "warn" | "bad" {
  if (dropped.frames === 0) return "ok";
  return droppedPercent(dropped) < 1 ? "warn" : "bad";
}

/** "512 MB", "1.5 GB" or "2.1 TB", from MB. */
export function formatMegabytes(mb: number): string {
  if (mb < 1024) return `${Math.round(mb)} MB`;
  if (mb < 1024 * 1024) return `${(mb / 1024).toFixed(1)} GB`;
  return `${(mb / 1024 / 1024).toFixed(1)} TB`;
}
