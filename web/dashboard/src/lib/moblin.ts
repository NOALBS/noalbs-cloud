import type { MoblinStatus, MoblinThermal } from "@/api/types";

/** Shorter passwords are refused (the API checks it too). */
export const MOBLIN_MIN_PASSWORD = 8;

/** A status older than this is gone: obs-link rewrites it at least every 10 s. */
const STALE_MS = 45_000;

export type MoblinView =
  /** Waiting for the live feed. */
  | { kind: "loading" }
  /** No Moblin is connected. */
  | { kind: "disconnected" }
  /** Connected, but the switcher is over its plan's limit. */
  | { kind: "paused"; status: MoblinStatus }
  | { kind: "connected"; status: MoblinStatus };

/** What the Moblin card should show. */
export function moblinView(status: MoblinStatus | null | undefined, now: number): MoblinView {
  if (status === undefined) return { kind: "loading" };
  if (status === null || now - Date.parse(status.updated_at) > STALE_MS) {
    return { kind: "disconnected" };
  }
  return status.paused ? { kind: "paused", status } : { kind: "connected", status };
}

/**
 * The phone's heat as Moblin's flame shows it (iOS gives apps no temperature, only
 * this); `null` if Moblin didn't say.
 */
export function thermalLabel(thermal: MoblinThermal | null | undefined): string | null {
  switch (thermal) {
    case "normal":
      return "Normal";
    case "serious":
      return "Getting hot";
    case "critical":
      return "Hot";
    default:
      return null;
  }
}

/** "87%" or "87% · charging"; `null` if Moblin didn't say. */
export function batteryLabel(status: MoblinStatus): string | null {
  if (status.battery_level == null) return null;
  return status.battery_charging
    ? `${status.battery_level}% · charging`
    : `${status.battery_level}%`;
}

/** The bitrate presets, lowest first. */
export function sortedPresets(status: MoblinStatus): MoblinStatus["bitrate_presets"] {
  return [...status.bitrate_presets].sort((a, b) => a.kbps - b.kbps);
}

/** Moblin's connection priorities range. */
export const MOBLIN_MIN_PRIORITY = 1;
export const MOBLIN_MAX_PRIORITY = 10;

/** Zoom factors the API passes on (Moblin clamps them to the camera's range). */
export const MOBLIN_MIN_ZOOM = 0.5;
export const MOBLIN_MAX_ZOOM = 100;

/**
 * Moblin's system monitor line ("12%/45% 310 MB": Moblin's CPU, the phone's CPU,
 * memory) spelled out; anything else as it came.
 */
export function systemMonitorLabel(text: string): string {
  const match = /^(\d+)%\/(\d+)%\s+(\d+)\s*MB$/.exec(text.trim());
  if (!match) return text;
  const [, app, cpu, memory] = match;
  return `CPU ${cpu}% (Moblin ${app}%) · ${memory} MB`;
}

/** One zoom step in (`1`) or out (`-1`): 25% at a time, to 0.1x, within the API's range. */
export function stepZoom(current: number, direction: 1 | -1): number {
  const next = direction > 0 ? current * 1.25 : current / 1.25;
  const rounded = Math.round(next * 10) / 10;
  return Math.min(MOBLIN_MAX_ZOOM, Math.max(MOBLIN_MIN_ZOOM, rounded));
}

/** "2.2x" */
export function zoomLabel(x: number): string {
  return `${Number(x.toFixed(1))}x`;
}

/** Whether connection `id` may be turned off: Moblin would use them all again with none on. */
export function canTurnOffConnection(
  connections: MoblinStatus["connections"],
  id: string,
): boolean {
  return connections.some((connection) => connection.id !== id && connection.enabled);
}

export interface BondingRow {
  name: string;
  /** Share of what's sent, %. */
  percent: number | null;
  /** Round-trip time, ms. */
  rttMs: number | null;
}

/**
 * Moblin's bonding lines ("94% WiFi, 6% IP15" and "35 ms WiFi, 101 ms IP15") as one
 * row per connection, in Moblin's order; `null` if they don't look like that.
 */
export function bondingRows(bonding: string | null, rtts: string | null): BondingRow[] | null {
  const parse = (text: string | null, pattern: RegExp) => {
    if (!text) return [];
    const entries = text.split(", ").map((entry) => pattern.exec(entry.trim()));
    if (entries.some((entry) => !entry)) return null;
    return entries.map((entry) => ({ value: Number(entry?.[1]), name: entry?.[2] ?? "" }));
  };
  const shares = parse(bonding, /^(\d+)% (.+)$/);
  const times = parse(rtts, /^(-?\d+) ms (.+)$/);
  if (!shares || !times || (shares.length === 0 && times.length === 0)) return null;

  const rows: BondingRow[] = shares.map(({ value, name }) => ({
    name,
    percent: value,
    rttMs: null,
  }));
  for (const { value, name } of times) {
    // Moblin says -1 before it has measured one.
    const rttMs = value < 0 ? null : value;
    const row = rows.find((candidate) => candidate.name === name);
    if (row) row.rttMs = rttMs;
    else rows.push({ name, percent: null, rttMs });
  }
  return rows;
}

export interface MoblinkRelay {
  name: string;
  battery: number | null;
  /** 🪫: 10% or less. */
  lowBattery: boolean;
  /** 🔥: the relay phone is too hot. */
  hot: boolean;
}

/**
 * Moblin's Moblink line ("IP15🔋34%, IP15-50🔥🪫8%") as relays; `null` if it isn't
 * a list of relays (e.g. this phone is itself a relay: "Connected").
 */
export function moblinkRelays(text: string): MoblinkRelay[] | null {
  const relays = text.split(", ").map((entry) => {
    const match = /^(.+?)(🔥)?(?:(🔋|🪫)(\d+)%)?$/u.exec(entry.trim());
    if (!match) return null;
    const [, name, hot, icon, battery] = match;
    return {
      name,
      battery: battery ? Number(battery) : null,
      lowBattery: icon === "🪫",
      hot: Boolean(hot),
    };
  });
  // Without a battery or flame on any entry it's no relay list.
  if (
    relays.some((relay) => !relay) ||
    !relays.some((relay) => relay?.battery != null || relay?.hot)
  ) {
    return null;
  }
  return relays as MoblinkRelay[];
}
