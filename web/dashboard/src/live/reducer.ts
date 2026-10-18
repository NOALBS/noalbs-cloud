import type {
  BelaboxStatus,
  FeedState,
  MoblinStatus,
  StatsSample,
  SwitcherEvent,
  SwitcherStatus,
} from "@/api/types";

/** How many seconds of stats the charts show (matches the API's replay). */
export const HISTORY_SECONDS = 120;
/** Kept past the left edge, so the chart's line always reaches it (the SVG clips the rest). */
export const HISTORY_MARGIN_SECONDS = 5;

export interface Point {
  /** Milliseconds since the epoch. */
  at: number;
  bitrate: number;
  rtt: number | null;
  state: FeedState;
}

export type Connection = "connecting" | "open" | "reconnecting";

export interface LiveState {
  connection: Connection;
  /** `null` while no browser source is connected; `undefined` before the first message. */
  status: SwitcherStatus | null | undefined;
  /** Recent samples per stream server, oldest first. */
  history: Record<string, Point[]>;
  /** Events received live, newest first. */
  events: SwitcherEvent[];
  /**
   * The BELABOX's status: `null` while no BELABOX link runs (no remote key, or no
   * browser source); `undefined` before the first message.
   */
  belabox: BelaboxStatus | null | undefined;
  /** Recent throughput per BELABOX network interface (`bitrate` in kbps), oldest first. */
  belaboxHistory: Record<string, Point[]>;
  /** The connected Moblin's status: `null` while none is connected; `undefined` before the first message. */
  moblin: MoblinStatus | null | undefined;
}

export type LiveAction =
  | { type: "connection"; connection: Connection }
  | { type: "status"; status: SwitcherStatus | null }
  | { type: "stats"; sample: StatsSample }
  | { type: "event"; event: SwitcherEvent }
  | { type: "belabox"; status: BelaboxStatus | null }
  | { type: "moblin"; status: MoblinStatus | null };

export const initialLiveState: LiveState = {
  connection: "connecting",
  status: undefined,
  history: {},
  events: [],
  belabox: undefined,
  belaboxHistory: {},
  moblin: undefined,
};

const MAX_EVENTS = 100;

export function liveReducer(state: LiveState, action: LiveAction): LiveState {
  switch (action.type) {
    case "connection":
      return { ...state, connection: action.connection };
    case "status":
      return { ...state, status: action.status };
    case "stats": {
      const at = Date.parse(action.sample.at);
      const cutoff = at - (HISTORY_SECONDS + HISTORY_MARGIN_SECONDS) * 1000;
      const history = { ...state.history };
      for (const server of action.sample.servers) {
        const points = history[server.server_id] ?? [];
        // A reconnect replays the last two minutes: skip samples we have.
        const last = points[points.length - 1];
        if (last && last.at >= at) continue;
        history[server.server_id] = [
          ...points.filter((point) => point.at > cutoff),
          { at, bitrate: server.bitrate, rtt: server.rtt ?? null, state: server.state },
        ];
      }
      return { ...state, history };
    }
    case "event": {
      if (state.events.some((event) => event.id === action.event.id)) return state;
      return { ...state, events: [action.event, ...state.events].slice(0, MAX_EVENTS) };
    }
    case "belabox":
      return {
        ...state,
        belabox: action.status,
        belaboxHistory: belaboxHistory(state.belaboxHistory, action.status),
      };
    case "moblin":
      return { ...state, moblin: action.status };
  }
}

/** Adds each interface's throughput from a BELABOX status; forgets interfaces that are gone. */
function belaboxHistory(
  history: Record<string, Point[]>,
  status: BelaboxStatus | null,
): Record<string, Point[]> {
  if (!status) return {};
  if (status.link !== "online") return history;
  const at = Date.parse(status.updated_at);
  const cutoff = at - (HISTORY_SECONDS + HISTORY_MARGIN_SECONDS) * 1000;
  const next: Record<string, Point[]> = {};
  for (const netif of status.interfaces) {
    const points = history[netif.name] ?? [];
    const last = points[points.length - 1];
    next[netif.name] =
      last && last.at >= at
        ? points
        : [
            ...points.filter((point) => point.at > cutoff),
            { at, bitrate: netif.kbps, rtt: null, state: netif.enabled ? "live" : "idle" },
          ];
  }
  return next;
}

/** The newest sample of a server, if any. */
export function latest(state: LiveState, serverId: string): Point | undefined {
  const points = state.history[serverId];
  return points?.[points.length - 1];
}

/**
 * Whether the status is too old to trust. obs-link rewrites it every 10 s, and
 * the key expires after 30 s if it stops; this also covers a stuck feed.
 */
export function isStale(status: SwitcherStatus, now = Date.now()): boolean {
  return now - Date.parse(status.updated_at) > 45_000;
}

/**
 * The time at a chart's right edge: the newest sample's own time, so the line
 * steps left exactly once per sample. (Using the browser clock made it jitter:
 * the clock and the samples tick out of step.) The clock is only a fallback.
 */
export function chartEnd(points: Point[], now: number): number {
  return points[points.length - 1]?.at ?? now;
}
