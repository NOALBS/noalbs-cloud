import { createElement } from "react";
import { beforeEach, describe, expect, it } from "vitest";
import {
  defaults,
  operator,
  operatorFor,
  paneDefinitions,
  read,
  researchOperatorSaved,
  rowHeight,
  samePaneBody,
  storageKey,
} from "./pane-layout";

describe("live pane layouts", () => {
  beforeEach(() => localStorage.clear());
  it("fills the viewport without collisions at every Operator breakpoint", () => {
    for (const cols of [3, 4, 5]) {
      const layout = defaults(operatorFor(cols), cols);
      expect(layout).toHaveLength(cols === 3 ? 7 : 9);
      expect(Math.max(...layout.map((p) => p.y + p.h))).toBe(12);
      for (let y = 0; y < 12; y++) {
        for (let x = 0; x < cols; x++) {
          expect(
            layout.filter((p) => x >= p.x && x < p.x + p.w && y >= p.y && y < p.y + p.h),
          ).toHaveLength(1);
        }
      }
      for (const item of defaults(
        paneDefinitions.map((p) => p.id),
        cols,
      )) {
        expect(item.x + item.w).toBeLessThanOrEqual(cols);
        expect(item.w).toBeGreaterThanOrEqual(item.minW ?? 0);
        expect(item.h).toBeGreaterThanOrEqual(item.minH ?? 0);
      }
    }
  });
  it("computes row height from available space including all grid gutters", () => {
    for (const available of [500, 700, 1200])
      expect(rowHeight(available) * 12 + 12 * 11).toBeCloseTo(available);
  });
  it("uses three, four and five readable columns, with media spanning two columns on wide screens", () => {
    for (const cols of [3, 4, 5]) {
      const layout = defaults(operatorFor(cols), cols);
      expect(Math.max(...layout.map((item) => item.x + item.w))).toBe(cols);
      for (const item of layout)
        expect(item.w).toBe(["scenes", "program-preview"].includes(item.i) ? cols - 1 : 1);
    }
    expect(operatorFor(3)).toEqual(
      expect.arrayContaining([
        "controls",
        "servers",
        "scenes",
        "sources",
        "activity",
        "program-preview",
        "bitrate-chart",
      ]),
    );
    for (const id of ["audio", "belabox", "moblin"]) expect(operatorFor(3)).not.toContain(id);
  });
  it("persists visibility per desktop breakpoint without changing other defaults", () => {
    const saved = read("fresh", false);
    expect(saved.visibleByCols?.[3]).toEqual(researchOperatorSaved().visibleByCols?.[3]);
    expect(saved.visibleByCols?.[4]).toEqual(researchOperatorSaved().visible);
    saved.visibleByCols = { ...saved.visibleByCols, 3: [] };
    localStorage.setItem("saved-visibility", JSON.stringify(saved));
    expect(read("saved-visibility", false).visibleByCols?.[3]).toEqual([]);
    expect(read("saved-visibility", false).visibleByCols?.[5]).toEqual(
      researchOperatorSaved().visible,
    );
  });
  it("defaults phones to Streamer order and keeps detailed and studio panes addable", () => {
    expect(read("phone", true).visible).toEqual([
      "status",
      "controls",
      "scenes",
      "servers",
      "belabox",
      "moblin",
      "sources",
      "audio",
      "activity",
    ]);
    expect(read("desktop", false).visible).toEqual(researchOperatorSaved().visible);
    expect(operator).toContain("program-preview");
    expect(operator).toContain("belabox");
    expect(operator).toContain("moblin");
    expect(operator).toContain("bitrate-chart");
    expect(operator).not.toContain("preview");
  });
  it("ignores the previous storage version and separates switchers and devices", () => {
    localStorage.setItem(
      "noalbs-live-layout:one:desktop",
      JSON.stringify({ visible: [], layouts: {} }),
    );
    expect(read(storageKey("one", false), false).visible).toEqual(researchOperatorSaved().visible);
    expect(storageKey("one", false)).not.toBe(storageKey("two", false));
    expect(storageKey("one", false)).not.toBe(storageKey("one", true));
  });
  it("preserves intentionally empty layouts", () => {
    localStorage.setItem("empty", JSON.stringify({ visible: [], layouts: {} }));
    expect(read("empty", false).visible).toEqual([]);
    expect(defaults([], 3)).toEqual([]);
  });
  it("restores addable panes and saved positions, falling back on malformed storage", () => {
    const saved = {
      visible: ["program-preview"],
      layouts: { 3: defaults(["program-preview"], 3) },
    };
    localStorage.setItem("saved", JSON.stringify(saved));
    expect(read("saved", false)).toEqual(saved);
    for (const invalid of [
      "{",
      '{"visible":["unknown"],"layouts":{}}',
      '{"visible":[],"layouts":{"12":[null]}}',
      '{"visible":["status","status"],"layouts":{}}',
      '{"visible":["status"],"layouts":{},"visibleByCols":{"12":["unknown"]}}',
      '{"visible":["status"],"layouts":{},"visibleByCols":{"12":null}}',
    ]) {
      localStorage.setItem("bad", invalid);
      expect(read("bad", false).visible).toEqual(researchOperatorSaved().visible);
    }
  });
  it("uses whole pane columns and minimum heights instead of narrow fractional cards", () => {
    for (const cols of [2, 3, 4, 5])
      for (const item of defaults(operatorFor(cols), cols)) {
        expect(Number.isInteger(item.x)).toBe(true);
        expect(item.minW).toBe(1);
        expect(item.minH).toBeGreaterThanOrEqual(2);
      }
  });
  it("memoizes equivalent pane props but never drops changed data, callbacks or empty states", () => {
    const pane = (props: Record<string, unknown>) => createElement("section", props);
    expect(samePaneBody(pane({ path: { id: "one" } }), pane({ path: { id: "one" } }))).toBe(true);
    expect(samePaneBody(pane({ path: { id: "one" } }), pane({ path: { id: "two" } }))).toBe(false);
    expect(samePaneBody(pane({ onClick: () => 1 }), pane({ onClick: () => 1 }))).toBe(false);
    expect(samePaneBody(null, null)).toBe(true);
    expect(samePaneBody(null, pane({}))).toBe(false);
    expect(samePaneBody(pane({ error: false }), pane({ error: true }))).toBe(false);
    expect(samePaneBody(pane({}), createElement("div"))).toBe(false);
  });
});
