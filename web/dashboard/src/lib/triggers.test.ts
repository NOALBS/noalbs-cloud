import { describe, expect, it } from "vitest";
import { bitrateRules, bitrateZones, rttZones, triggerError } from "./triggers";

const triggers = (
  low: number | null,
  offline: number | null,
  rtt = 2500,
  rtt_offline: number | null = null,
) => ({
  low,
  offline,
  rtt,
  rtt_offline,
});

describe("triggerError", () => {
  it("accepts defaults and off triggers", () => {
    expect(triggerError(triggers(800, null))).toBeNull();
    expect(triggerError(triggers(null, null))).toBeNull();
    expect(triggerError(triggers(800, 150, 2500, 5000))).toBeNull();
  });

  it("matches the API's rules", () => {
    expect(triggerError(triggers(0, null))).toMatch(/between 1 and/);
    expect(triggerError(triggers(800, 800))).toMatch(/lower than the LOW/);
    expect(triggerError(triggers(800, null, 2500, 2000))).toMatch(/higher than the LOW/);
    expect(triggerError(triggers(800.5, null))).toMatch(/between 1 and/);
  });
});

describe("zones", () => {
  it("splits the bitrate bar at the triggers", () => {
    expect(bitrateZones({ low: 800, offline: 400 })).toEqual([
      { role: "offline", from: 0, to: 5 },
      { role: "low", from: 5, to: 10 },
      { role: "live", from: 10, to: 100 },
    ]);
    expect(bitrateZones({ low: null, offline: null })).toEqual([
      { role: "live", from: 0, to: 100 },
    ]);
  });

  it("puts high RTT on the right", () => {
    expect(rttZones({ rtt: 2500, rtt_offline: null })).toEqual([
      { role: "live", from: 0, to: 50 },
      { role: "low", from: 50, to: 100 },
    ]);
  });
});

describe("bitrateRules", () => {
  it("describes each range", () => {
    expect(bitrateRules({ low: 800, offline: 150 })).toEqual([
      { role: "live", range: "above 800 kbps" },
      { role: "low", range: "151 kbps to 800 kbps" },
      { role: "offline", range: "150 kbps or less, or no feed" },
    ]);
    expect(bitrateRules({ low: 800, offline: null })[2]).toEqual({
      role: "offline",
      range: "no feed",
    });
  });
});
