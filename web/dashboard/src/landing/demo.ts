// The landing page's Live-page mock: a made-up stream that loses signal and gets
// it back, switched by the same rules as the real switcher (simplified).

export type DemoState = "live" | "low" | "offline";

/** The Live page's looks (`components/live/feed-state.ts`), as plain class strings. */
export interface Look {
  label: string;
  text: string;
  soft: string;
  border: string;
  fill: string;
}
export const LOOKS: Record<DemoState, Look> = {
  live: {
    label: "LIVE",
    text: "text-live",
    soft: "bg-live/12",
    border: "border-live/35",
    fill: "bg-live",
  },
  low: {
    label: "LOW BITRATE",
    text: "text-low",
    soft: "bg-low/12",
    border: "border-low/40",
    fill: "bg-low",
  },
  offline: {
    label: "OFFLINE",
    text: "text-offline",
    soft: "bg-offline/14",
    border: "border-offline/45",
    fill: "bg-offline",
  },
};

/** The OBS scene each state switches to, as named in the mock's scene grid. */
export const SCENES: Record<DemoState, string> = {
  live: "IRL cam",
  low: "Low bitrate",
  offline: "BRB",
};
/** The stream server the demo reads. */
export const SERVER = "BELABOX Cloud";

/** The bitrate charts' width in SVG units and the seconds they show. */
export const CHART_WIDTH = 760;
export const CHART_SECONDS = 60;

/** Bitrate as a line and an area for an SVG `height` units tall, scaled like `BitrateChart`. */
export function chartPoints(values: number[], height: number) {
  const pad = 6;
  const max = Math.max(8000, ...values.map((value) => value * 1.1));
  const step = CHART_WIDTH / (CHART_SECONDS - 1);
  const offset = CHART_SECONDS - values.length;
  const y = (value: number) => height - pad - (Math.min(value, max) / max) * (height - pad * 2);
  const line = values.map(
    (value, i) => `${((i + offset) * step).toFixed(1)},${y(value).toFixed(1)}`,
  );
  const first = (offset * step).toFixed(1);
  return {
    line: line.join(" "),
    area: `${first},${height} ${line.join(" ")} ${CHART_WIDTH},${height}`,
    y,
  };
}

/** The mocks' stream has been live this long when the page opens. */
export const UPTIME_START = 1 * 3600 + 24 * 60 + 7;

/** Seconds as H:MM:SS, like the Live page's uptime. */
export function formatUptime(seconds: number): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${Math.floor(seconds / 3600)}:${pad(Math.floor((seconds % 3600) / 60))}:${pad(seconds % 60)}`;
}

/** The mock's triggers, as on a typical switcher. */
export const LOW_KBPS = 2500;
export const OFFLINE_KBPS = 300;
/** A new state must hold this many *more* checks, like `retry_attempts`. */
export const RETRY_ATTEMPTS = 2;

interface Leg {
  seconds: number;
  /** Bitrate range in kbps; `null` = no feed. */
  kbps: [number, number] | null;
  rtt: [number, number];
}

/** One loop: downtown, up the hill, the tunnel, out the other side. */
export const ROUTE: Leg[] = [
  { seconds: 10, kbps: [5700, 6500], rtt: [60, 130] },
  { seconds: 8, kbps: [420, 960], rtt: [520, 1100] },
  { seconds: 7, kbps: null, rtt: [0, 0] },
  { seconds: 7, kbps: [5600, 6400], rtt: [70, 150] },
];
export const LOOP_SECONDS = ROUTE.reduce((sum, leg) => sum + leg.seconds, 0);

export interface Reading {
  kbps: number | null;
  rtt: number | null;
}

/** What the stream server reports at second `t` of the loop. */
export function readingAt(t: number, random: () => number = Math.random): Reading {
  let at = ((t % LOOP_SECONDS) + LOOP_SECONDS) % LOOP_SECONDS;
  for (const leg of ROUTE) {
    if (at < leg.seconds) {
      if (!leg.kbps) return { kbps: null, rtt: null };
      const pick = ([min, max]: [number, number]) => min + random() * (max - min);
      return { kbps: Math.round(pick(leg.kbps) / 10) * 10, rtt: Math.round(pick(leg.rtt)) };
    }
    at -= leg.seconds;
  }
  return { kbps: null, rtt: null };
}

export function feedState(reading: Reading): DemoState {
  if (reading.kbps === null || reading.kbps < OFFLINE_KBPS) return "offline";
  return reading.kbps < LOW_KBPS ? "low" : "live";
}

export interface Switcher {
  state: DemoState;
  pending: DemoState | null;
  count: number;
}

export const START: Switcher = { state: "live", pending: null, count: 0 };

/** A repeatable random series, so the history sketch looks the same on every visit. */
export function seeded(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 2 ** 32;
  };
}

export interface HistoryStep {
  min: number;
  avg: number;
  max: number;
  state: DemoState;
}

/**
 * A made-up 3-hour stream in 90 steps for the history sketch: mostly LIVE, a
 * rough patch, a dead zone and a long LOW stretch, like the History page shows.
 */
export function historySketch(): HistoryStep[] {
  const random = seeded(7);
  const rough = (step: number) =>
    (step >= 22 && step < 27) || (step >= 58 && step < 68) ? 1 : step >= 40 && step < 43 ? 2 : 0;
  return Array.from({ length: 90 }, (_, step) => {
    const level = rough(step);
    const avg =
      level === 2
        ? 0
        : level === 1
          ? 900 + random() * 900
          : 5400 + random() * 900 - (step % 9) * 30;
    const spread = level === 2 ? 0 : 400 + random() * (level ? 700 : 1300);
    const min = Math.max(0, avg - spread);
    const max = avg + spread * 0.6;
    const state: DemoState = level === 2 ? "offline" : min < LOW_KBPS && level ? "low" : "live";
    return { min, avg, max, state };
  });
}

/**
 * One check: a new feed state switches once it held `RETRY_ATTEMPTS` more
 * checks, except coming back from offline, which is instant (`instant_recover`).
 */
export function check(switcher: Switcher, reading: Reading): Switcher {
  const feed = feedState(reading);
  if (feed === switcher.state) return { ...switcher, pending: null, count: 0 };
  if (switcher.state === "offline") return { state: feed, pending: null, count: 0 };
  const count = switcher.pending === feed ? switcher.count + 1 : 0;
  if (count >= RETRY_ATTEMPTS) return { state: feed, pending: null, count: 0 };
  return { state: switcher.state, pending: feed, count };
}
