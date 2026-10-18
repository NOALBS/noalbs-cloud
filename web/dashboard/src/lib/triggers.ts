import type { Triggers } from "@/api/types";
import { currentLocale } from "@/lib/locale";

/** The bitrate bar ends here; higher bitrates show at its right edge. */
export const BITRATE_SCALE_MAX = 8000;
/** The RTT bar ends here. */
export const RTT_SCALE_MAX = 5000;

export const TRIGGER_LIMITS = {
  bitrate: { min: 1, max: 100_000 },
  rtt: { min: 1, max: 60_000 },
} as const;

/** Mirrors the API's rules; returns a message for the first problem found. */
export function triggerError(triggers: Triggers): string | null {
  const inRange = (value: number | null | undefined, max: number) =>
    value == null || (Number.isInteger(value) && value >= 1 && value <= max);

  if (!inRange(triggers.low, 100_000) || !inRange(triggers.offline, 100_000)) {
    return "Bitrate triggers must be between 1 and 100,000 kbps.";
  }
  if (!inRange(triggers.rtt, 60_000) || !inRange(triggers.rtt_offline, 60_000)) {
    return "RTT triggers must be between 1 and 60,000 ms.";
  }
  if (triggers.low != null && triggers.offline != null && triggers.offline >= triggers.low) {
    return "The BRB bitrate must be lower than the LOW bitrate.";
  }
  if (
    triggers.rtt != null &&
    triggers.rtt_offline != null &&
    triggers.rtt_offline <= triggers.rtt
  ) {
    return "The BRB RTT must be higher than the LOW RTT.";
  }
  return null;
}

/** Percentage position of `value` on a bar that ends at `max`. */
export function scalePercent(value: number, max: number): number {
  return (Math.min(Math.max(value, 0), max) / max) * 100;
}

export type SceneRole = "live" | "low" | "offline";

export interface Zone {
  role: SceneRole;
  /** Start and end, as percentages of the bar. */
  from: number;
  to: number;
}

/**
 * Coloured zones for the bitrate bar: BRB below the offline trigger, LOW below
 * the low trigger, LIVE above. Off triggers simply have no zone.
 */
export function bitrateZones(triggers: Pick<Triggers, "low" | "offline">): Zone[] {
  const offline = triggers.offline ? scalePercent(triggers.offline, BITRATE_SCALE_MAX) : 0;
  const low = triggers.low ? scalePercent(triggers.low, BITRATE_SCALE_MAX) : offline;
  return [
    { role: "offline" as const, from: 0, to: offline },
    { role: "low" as const, from: offline, to: Math.max(low, offline) },
    { role: "live" as const, from: Math.max(low, offline), to: 100 },
  ].filter((zone) => zone.to > zone.from);
}

/** Zones for the RTT bar, where higher is worse: LIVE, then LOW, then BRB. */
export function rttZones(triggers: Pick<Triggers, "rtt" | "rtt_offline">): Zone[] {
  const low = triggers.rtt ? scalePercent(triggers.rtt, RTT_SCALE_MAX) : 100;
  const offline = triggers.rtt_offline ? scalePercent(triggers.rtt_offline, RTT_SCALE_MAX) : 100;
  return [
    { role: "live" as const, from: 0, to: Math.min(low, offline) },
    { role: "low" as const, from: Math.min(low, offline), to: offline },
    { role: "offline" as const, from: offline, to: 100 },
  ].filter((zone) => zone.to > zone.from);
}

const kbps = (value: number) => `${value.toLocaleString(currentLocale())} kbps`;

/** Which scene each bitrate range leads to, in words, e.g. "800+ kbps → LIVE". */
export function bitrateRules(
  triggers: Pick<Triggers, "low" | "offline">,
): { role: SceneRole; range: string }[] {
  const { low, offline } = triggers;
  const rules: { role: SceneRole; range: string }[] = [];

  rules.push({ role: "live", range: low ? `above ${kbps(low)}` : "any bitrate" });
  if (low) {
    rules.push({
      role: "low",
      range: offline ? `${kbps(offline + 1)} to ${kbps(low)}` : `${kbps(low)} or less`,
    });
  }
  rules.push({
    role: "offline",
    range: offline ? `${kbps(offline)} or less, or no feed` : "no feed",
  });
  return rules;
}
