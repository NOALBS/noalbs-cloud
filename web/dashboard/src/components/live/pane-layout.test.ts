import { beforeEach, describe, expect, it } from "vitest";
import { columnsFor, fitPreset, paneDefinitions, preset, read, storageKey } from "./pane-layout";

const ids = paneDefinitions.map((pane) => pane.id) as string[];

describe("pane layout", () => {
  beforeEach(() => localStorage.clear());

  it("gives wider screens more columns", () => {
    expect([800, 1150, 1620, 2100, 3400].map(columnsFor)).toEqual([12, 12, 24, 36, 36]);
  });

  it.each(["streamer", "operator"] as const)(
    "has %s presets that fit and don't overlap",
    (mode) => {
      for (const cols of [12, 24, 36]) {
        const layout = preset(mode, cols);
        expect(layout.length).toBeGreaterThan(0);
        for (const a of layout) {
          expect(ids).toContain(a.i);
          expect(a.x + a.w).toBeLessThanOrEqual(cols);
          for (const b of layout)
            if (a !== b)
              expect(a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h).toBe(
                false,
              );
        }
      }
    },
  );

  it("keeps Program/Preview out of streamer mode", () => {
    for (const cols of [12, 24, 36])
      expect(preset("streamer", cols).map((item) => item.i)).not.toContain("monitors");
  });

  it("includes Streamer audio and OBS health in complete rows without holes", () => {
    for (const cols of [12, 24, 36]) {
      const layout = preset("streamer", cols);
      expect(layout.map((item) => item.i)).toEqual(expect.arrayContaining(["audio", "obs"]));
      const height = Math.max(...layout.map((item) => item.y + item.h));
      expect(layout.reduce((area, item) => area + item.w * item.h, 0)).toBe(cols * height);
    }
    expect(storageKey("s1", "streamer")).toContain(":v22:");
    expect(storageKey("s1", "operator")).toContain(":v21:");
  });

  it.each([12, 24, 36])("keeps default rows complete when content grows at %s columns", (cols) => {
    const layout = preset("streamer", cols);
    const needs = Object.fromEntries(
      layout.map((item, index) => [item.i, item.h + (index % 3) * 4]),
    );
    const fitted = fitPreset(layout, needs);
    expect(layout).toEqual(preset("streamer", cols));
    const height = Math.max(...fitted.map((item) => item.y + item.h));
    expect(fitted.reduce((area, item) => area + item.w * item.h, 0)).toBe(cols * height);
    for (const a of fitted) {
      expect(a.h).toBeGreaterThanOrEqual(needs[a.i] ?? 0);
      for (const b of fitted)
        if (a !== b)
          expect(a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h).toBe(
            false,
          );
    }
  });

  it("leaves empty layouts and invalid or absent size requests alone", () => {
    expect(fitPreset([], { preview: 30 })).toEqual([]);
    const layout = preset("streamer", 24);
    expect(
      fitPreset(layout, { preview: Number.NaN, audio: Number.POSITIVE_INFINITY, absent: 50 }),
    ).toEqual(layout);
  });

  it("reads a saved layout and ignores a broken one", () => {
    const key = storageKey("s1", "operator");
    expect(key).not.toBe(storageKey("s1", "streamer"));
    const saved = { phone: ["scenes", "bitrate"], grids: { 24: preset("operator", 24) } };
    localStorage.setItem(key, JSON.stringify(saved));
    expect(read(key)).toEqual(saved);
    localStorage.setItem(
      key,
      JSON.stringify({ grids: { 24: [{ i: "nope", x: 0, y: 0, w: 1, h: 1 }] } }),
    );
    expect(read(key)).toEqual({ grids: {} });
    localStorage.setItem(key, "{");
    expect(read(key)).toEqual({ grids: {} });
  });
});
