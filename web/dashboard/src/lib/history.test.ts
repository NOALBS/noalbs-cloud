import { describe, expect, it } from "vitest";
import {
  bandPath,
  flattenNumbers,
  formatDuration,
  formatOffset,
  groupByDay,
  linePath,
  markersToText,
  niceCeil,
  numericFields,
  pointAt,
  segments,
  stateShares,
  timeTicks,
  worstState,
} from "./history";

const states = (live: number, low: number, offline: number) => ({
  live_seconds: live,
  low_seconds: low,
  offline_seconds: offline,
});

describe("formatDuration", () => {
  it("shows the two largest units", () => {
    expect(formatDuration(4 * 3600 + 12 * 60 + 30)).toBe("4h 12m");
    expect(formatDuration(12 * 60 + 5)).toBe("12m 5s");
    expect(formatDuration(45)).toBe("45s");
    expect(formatDuration(-3)).toBe("0s");
  });
});

describe("stateShares", () => {
  it("splits the time between states", () => {
    expect(stateShares(states(75, 20, 5))).toEqual({ live: 0.75, low: 0.2, offline: 0.05 });
  });

  it("is all zero with nothing recorded", () => {
    expect(stateShares(states(0, 0, 0))).toEqual({ live: 0, low: 0, offline: 0 });
  });
});

describe("worstState", () => {
  it("shows a single second of BRB over a mostly live step", () => {
    expect(worstState(states(29, 0, 1))).toBe("offline");
    expect(worstState(states(20, 10, 0))).toBe("low");
    expect(worstState(states(30, 0, 0))).toBe("live");
    expect(worstState(states(0, 0, 0))).toBe("idle");
  });
});

describe("groupByDay", () => {
  it("groups consecutive streams on the same local day", () => {
    const at = (day: number, hour: number) => new Date(2026, 8, day, hour).toISOString();
    const groups = groupByDay([
      { started_at: at(24, 21) },
      { started_at: at(24, 14) },
      { started_at: at(22, 20) },
    ]);
    expect(groups.map((group) => group.streams.length)).toEqual([2, 1]);
    expect(groups[0]?.label).toBe("Thursday, September 24");
  });
});

describe("flattenNumbers", () => {
  it("finds every number by its path", () => {
    const raw = {
      publishers: { live: { bitrate: 5800, rtt: 84.5, name: "live", ok: true } },
      streams: [{ pktRcvLoss: 3 }],
      version: "1.2",
    };
    expect(flattenNumbers(raw)).toEqual({
      "publishers.live.bitrate": 5800,
      "publishers.live.rtt": 84.5,
      "streams.0.pktRcvLoss": 3,
    });
  });

  it("collects the fields seen in any snapshot", () => {
    expect(numericFields([{ b: 1 }, { a: 2, b: 3 }, null])).toEqual(["a", "b"]);
  });
});

describe("pointAt", () => {
  const points = [0, 10, 20, 40].map((s) => ({ at: s * 1000 }));

  it("finds the step containing a time", () => {
    expect(pointAt(points, 15_000, 10_000)?.at).toBe(10_000);
    expect(pointAt(points, 40_000, 10_000)?.at).toBe(40_000);
  });

  it("finds nothing in a gap or outside", () => {
    expect(pointAt(points, 35_000, 10_000)).toBeUndefined();
    expect(pointAt(points, -1, 10_000)).toBeUndefined();
    expect(pointAt(points, 50_000, 10_000)).toBeUndefined();
  });
});

describe("chart paths", () => {
  const x = (at: number) => at / 1000;
  const y = (value: number) => 100 - value;
  const points = [0, 10, 20, 50, 60].map((s) => ({
    at: s * 1000,
    value: s,
    min: s - 1,
    max: s + 1,
  }));

  it("breaks runs at gaps", () => {
    expect(segments(points, 10_000).map((run) => run.length)).toEqual([3, 2]);
  });

  it("draws one subpath per run, through the middle of each step", () => {
    expect(linePath(points, 10_000, x, y)).toBe(
      "M5.0,100.0L15.0,90.0L25.0,80.0M55.0,50.0L65.0,40.0",
    );
  });

  it("closes a band per run", () => {
    const band = bandPath(points.slice(0, 2), 10_000, x, y);
    expect(band).toBe("M5.0,99.0L15.0,89.0L15.0,91.0L5.0,101.0Z");
  });
});

describe("niceCeil", () => {
  it("rounds up to a round number", () => {
    expect(niceCeil(6340)).toBe(8000);
    expect(niceCeil(1200)).toBe(1500);
    expect(niceCeil(84)).toBe(100);
    expect(niceCeil(0)).toBe(1);
  });
});

describe("timeTicks", () => {
  it("picks a round step that fits", () => {
    const from = new Date(2026, 8, 24, 20, 3).getTime();
    const ticks = timeTicks(from, from + 4 * 3600_000, 6);
    expect(ticks.length).toBeLessThanOrEqual(6);
    expect(ticks.map((at) => new Date(at).getMinutes())).toEqual(ticks.map(() => 0));
    expect(new Date(ticks[0] ?? 0).getHours()).toBe(21);
  });
});

describe("formatOffset", () => {
  it("drops the hour until there is one", () => {
    expect(formatOffset(0)).toBe("0:00");
    expect(formatOffset(247)).toBe("4:07");
    expect(formatOffset(3725)).toBe("1:02:05");
    expect(formatOffset(-5)).toBe("0:00");
  });
});

describe("markersToText", () => {
  const marker = (offset_seconds: number, description: string | null) => ({
    id: String(offset_seconds),
    at: "2026-10-13T20:00:00Z",
    offset_seconds,
    description,
    created_by: null,
  });

  it("writes a line per marker, in order", () => {
    expect(markersToText([marker(0, "Intro"), marker(3725, null)])).toBe(
      "0:00 Intro\n1:02:05 Marker",
    );
    expect(markersToText([])).toBe("");
  });
});
