import { describe, expect, it } from "vitest";
import {
  ALL_DEVICES,
  addTile,
  clampSpan,
  columnCount,
  effectiveTiles,
  hiddenTiles,
  hideTile,
  layoutStorageKey,
  minSpan,
  moveTile,
  normalize,
  normalizePhone,
  PRESETS,
  parseLayout,
  parsePhoneLayout,
  phoneStorageKey,
  resizeTile,
  serializeLayout,
  serializePhoneLayout,
} from "./layout";

describe("desktop layout", () => {
  it("has six valid presets and the wide breakpoint", () => {
    for (const cols of [12, 16] as const)
      for (const preset of Object.values(PRESETS[cols])) {
        expect(new Set(preset.map((tile) => tile.id)).size).toBe(preset.length);
        for (const tile of preset) {
          expect(tile.span).toBeGreaterThanOrEqual(minSpan(tile.id));
          expect(tile.span).toBeLessThanOrEqual(cols);
        }
      }
    expect(columnCount(1919)).toBe(12);
    expect(columnCount(1920)).toBe(16);
    expect(clampSpan("hero", 1, 12)).toBe(6);
    expect(clampSpan("sources", 99, 12)).toBe(12);
    expect(clampSpan("audio", 4.6, 16)).toBe(5);
    expect(clampSpan("audio", Number.NaN, 12)).toBe(3);
  });
  it.each([null, "{", "null", "[]", "1", '{"v":0}', '{"v":2}'])(
    "ignores malformed/versioned data: %s",
    (raw) => {
      expect(parseLayout(raw, ALL_DEVICES)).toBeNull();
    },
  );
  it("normalizes stale grids and preserves intentionally empty grids", () => {
    const layout = parseLayout(
      JSON.stringify({
        v: 1,
        preset: "old",
        grids: {
          12: [
            { id: "unknown", span: 3 },
            { id: "hero", span: 40 },
            { id: "hero", span: 6 },
            { id: "sources", span: 2 },
            { id: "audio", span: 4.5 },
            null,
          ],
          16: [],
        },
      }),
      ALL_DEVICES,
    );
    expect(layout).toEqual({
      v: 1,
      preset: "default",
      grids: {
        12: [
          { id: "hero", span: 12 },
          { id: "sources", span: 4 },
          { id: "audio", span: 5 },
        ],
        16: [],
      },
    });
    expect(parseLayout(serializeLayout(layout as NonNullable<typeof layout>), ALL_DEVICES)).toEqual(
      layout,
    );
    expect(effectiveTiles(layout, 16, ALL_DEVICES)).toEqual([]);
    expect(
      effectiveTiles(parseLayout('{"v":1,"grids":{"12":false}}', ALL_DEVICES), 12, ALL_DEVICES),
    ).toEqual(PRESETS[12].default);
  });
  it("clamps a wide grid shown narrow and drops unavailable device tiles", () => {
    expect(normalize([{ id: "hero", span: 16 }], 12, ALL_DEVICES)).toEqual([
      { id: "hero", span: 12 },
    ]);
    const availability = { belabox: false, moblin: false };
    expect(
      effectiveTiles(null, 16, availability).some(
        (tile) => tile.id === "belabox" || tile.id === "moblin",
      ),
    ).toBe(false);
    expect(effectiveTiles(null, 12, availability).some((tile) => tile.id === "devices")).toBe(true);
    expect(normalize(null, 12, availability)).toEqual([]);
    expect(
      parseLayout('{"v":1,"grids":{"16":[{"id":"moblin","span":4}]}}', availability)?.grids[16],
    ).toEqual([]);
  });
  it("moves, resizes, hides and adds without duplicates", () => {
    const initial = effectiveTiles(null, 12, ALL_DEVICES);
    expect(moveTile(initial, "hero", 2)[2]?.id).toBe("hero");
    expect(moveTile(initial, "obs", 1)).toEqual(initial);
    expect(resizeTile(initial, "hero", 30, 12)[0]?.span).toBe(12);
    const hidden = hideTile(initial, "hero");
    expect(hiddenTiles(hidden, ALL_DEVICES)).toContain("hero");
    expect(addTile(hidden, "hero", 12, ALL_DEVICES).at(-1)).toEqual({ id: "hero", span: 6 });
    expect(addTile(initial, "hero", 12, ALL_DEVICES)).toEqual(initial);
    expect(layoutStorageKey("u", "s")).toBe("noalbs-live-layout:u:s");
  });
});
describe("phone layout", () => {
  it.each([null, "{", "[]", '{"v":0}', '{"v":2}'])(
    "ignores malformed/versioned data: %s",
    (raw) => {
      expect(parsePhoneLayout(raw)).toBeNull();
    },
  );
  it("keeps order and hidden flags, drops stale ids/duplicates, appends missing sections", () => {
    const layout = parsePhoneLayout(
      '{"v":1,"sections":[{"id":"activity","hidden":true},{"id":"old"},{"id":"activity","hidden":false},{"id":"scenes","hidden":"yes"}]}',
    );
    expect(layout?.sections).toEqual([
      { id: "activity", hidden: true },
      { id: "scenes", hidden: false },
      { id: "servers", hidden: false },
      { id: "devices", hidden: false },
    ]);
    expect(parsePhoneLayout(serializePhoneLayout(layout as NonNullable<typeof layout>))).toEqual(
      layout,
    );
    expect(normalizePhone([])).toHaveLength(4);
    expect(normalizePhone(null)).toEqual(normalizePhone([]));
    expect(phoneStorageKey("u", "s")).toBe("noalbs-live-phone:u:s");
  });
});
