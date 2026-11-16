import { beforeEach, describe, expect, it } from "vitest";
import { columnsFor, paneDefinitions, preset, read, storageKey } from "./pane-layout";

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
