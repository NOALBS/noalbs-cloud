import { describe, expect, it } from "vitest";
import type { BelaboxStatus, StatsSample, SwitcherEvent, SwitcherStatus } from "@/api/types";
import { chartEnd, initialLiveState, isStale, latest, liveReducer } from "./reducer";

const sample = (seconds: number, bitrate: number): StatsSample => ({
  at: new Date(Date.UTC(2026, 8, 24, 12, 0, seconds)).toISOString(),
  servers: [{ server_id: "a", bitrate, rtt: 180, state: "live" }],
});

describe("liveReducer", () => {
  it("keeps two minutes of samples per server, plus a margin", () => {
    let state = initialLiveState;
    for (let second = 0; second < 150; second++) {
      const at = new Date(Date.UTC(2026, 8, 24, 12, 0, 0) + second * 1000).toISOString();
      state = liveReducer(state, {
        type: "stats",
        sample: { at, servers: [{ server_id: "a", bitrate: second, state: "live" }] },
      });
    }
    const points = state.history.a ?? [];
    expect(points.length).toBe(125);
    expect(points[0]?.bitrate).toBe(25);
    expect(latest(state, "a")?.bitrate).toBe(149);
  });

  it("ignores samples replayed after a reconnect", () => {
    let state = liveReducer(initialLiveState, { type: "stats", sample: sample(5, 6000) });
    state = liveReducer(state, { type: "stats", sample: sample(4, 100) });
    state = liveReducer(state, { type: "stats", sample: sample(5, 100) });
    expect(state.history.a?.map((point) => point.bitrate)).toEqual([6000]);
  });

  it("adds each event once, newest first", () => {
    const event = (id: string): SwitcherEvent => ({
      id,
      created_at: "2026-09-24T12:00:00Z",
      kind: "obs_connected",
    });
    let state = liveReducer(initialLiveState, { type: "event", event: event("1") });
    state = liveReducer(state, { type: "event", event: event("2") });
    state = liveReducer(state, { type: "event", event: event("1") });
    expect(state.events.map((e) => e.id)).toEqual(["2", "1"]);
  });

  it("keeps BELABOX throughput per interface while online", () => {
    const belabox = (seconds: number, kbps: number, link: BelaboxStatus["link"] = "online") => ({
      type: "belabox" as const,
      status: {
        updated_at: new Date(Date.UTC(2026, 8, 24, 12, 0, seconds)).toISOString(),
        link,
        streaming: true,
        total_kbps: kbps,
        interfaces: [{ name: "usb0", kbps, enabled: true }],
        sensors: [],
        notifications: [],
      },
    });
    let state = liveReducer(initialLiveState, belabox(1, 2000));
    state = liveReducer(state, belabox(2, 2500));
    // A heartbeat with the same time adds nothing; offline keeps the chart.
    state = liveReducer(state, belabox(2, 2500));
    state = liveReducer(state, belabox(3, 0, "encoder_offline"));
    expect(state.belabox?.link).toBe("encoder_offline");
    expect(state.belaboxHistory.usb0?.map((point) => point.bitrate)).toEqual([2000, 2500]);

    state = liveReducer(state, { type: "belabox", status: null });
    expect(state.belabox).toBeNull();
    expect(state.belaboxHistory).toEqual({});
  });

  it("tracks status and connection", () => {
    let state = liveReducer(initialLiveState, { type: "status", status: null });
    expect(state.status).toBeNull();
    state = liveReducer(state, { type: "connection", connection: "open" });
    expect(state.connection).toBe("open");
  });
});

describe("isStale", () => {
  it("flags statuses not refreshed for 45 seconds", () => {
    const status = { updated_at: "2026-09-24T12:00:00Z" } as SwitcherStatus;
    expect(isStale(status, Date.parse("2026-09-24T12:00:30Z"))).toBe(false);
    expect(isStale(status, Date.parse("2026-09-24T12:01:00Z"))).toBe(true);
  });
});

describe("chartEnd", () => {
  it("ends at the newest sample, not the browser clock", () => {
    const points = [
      { at: 1_000, bitrate: 1, rtt: null, state: "live" as const },
      { at: 2_000, bitrate: 2, rtt: null, state: "live" as const },
    ];
    expect(chartEnd(points, 2_700)).toBe(2_000);
    expect(chartEnd([], 2_700)).toBe(2_700);
  });
});
