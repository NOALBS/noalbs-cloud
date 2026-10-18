import { describe, expect, it } from "vitest";
import type { ObsAudio } from "@/api/types";
import {
  faderDb,
  faderPosition,
  formatDb,
  meterFraction,
  SILENT_METER,
  stepMeter,
  withMuted,
  withVolume,
} from "./audio";

describe("meterFraction", () => {
  it("maps -60…0 dB onto the meter, clamped", () => {
    expect(meterFraction(-60)).toBe(0);
    expect(meterFraction(-30)).toBe(0.5);
    expect(meterFraction(0)).toBe(1);
    expect(meterFraction(-100)).toBe(0);
    expect(meterFraction(3)).toBe(1);
  });
});

describe("formatDb", () => {
  it("reads like OBS", () => {
    expect(formatDb(0)).toBe("0.0 dB");
    expect(formatDb(-3.46)).toBe("-3.5 dB");
    expect(formatDb(-100)).toBe("-inf dB");
  });
});

describe("fader", () => {
  it("follows OBS's log curve", () => {
    expect(faderPosition(0)).toBe(1);
    expect(faderPosition(-96)).toBe(0);
    expect(faderPosition(-100)).toBe(0);
    expect(faderPosition(6)).toBe(1);
    // OBS puts -6 dB at about three quarters of the travel.
    expect(faderPosition(-6)).toBeCloseTo(0.755, 3);
    expect(faderDb(1)).toBe(0);
    expect(faderDb(0)).toBe(-100);
  });

  it("round-trips positions and decibels", () => {
    for (const db of [-60, -30, -12.5, -6, -1.2, -0.1]) {
      expect(faderDb(faderPosition(db))).toBe(db);
    }
  });
});

describe("stepMeter", () => {
  it("jumps up at once and falls at OBS's rate", () => {
    const loud = stepMeter(SILENT_METER, { peak: -6, magnitude: -12 }, 16, 1000);
    expect(loud.peak).toBe(-6);
    expect(loud.magnitude).toBe(-12);

    const falling = stepMeter(loud, undefined, 1000, 2000);
    expect(falling.peak).toBeCloseTo(-29.53);
    expect(falling.magnitude).toBeCloseTo(-35.53);
  });

  it("holds the highest peak for a while, then lets it fall", () => {
    const loud = stepMeter(SILENT_METER, { peak: -6, magnitude: -12 }, 16, 1000);
    const quieter = stepMeter(loud, { peak: -20, magnitude: -24 }, 100, 1100);
    expect(quieter.hold).toBe(-6);
    expect(quieter.peak).toBeCloseTo(-8.35);

    const later = stepMeter(quieter, { peak: -20, magnitude: -24 }, 1000, 3000);
    expect(later.hold).toBeLessThan(-6);
    expect(later.hold).toBeGreaterThanOrEqual(later.peak);
  });
});

describe("mixer list updates", () => {
  const list: ObsAudio = {
    scene: "LIVE",
    inputs: [
      { name: "Mic/Aux", global: true, muted: false, volume_db: 0 },
      { name: "BELABOX", global: false, muted: false, volume_db: -3 },
    ],
  };

  it("changes only the named input", () => {
    expect(withMuted(list, "Mic/Aux", true).inputs.map((input) => input.muted)).toEqual([
      true,
      false,
    ]);
    expect(withVolume(list, "BELABOX", -10).inputs.map((input) => input.volume_db)).toEqual([
      0, -10,
    ]);
  });
});
