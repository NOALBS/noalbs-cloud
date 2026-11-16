import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  defaults,
  type Row,
  read,
  reduceRows,
  resizePair,
  slotWidths,
  storageKey,
  valid,
  wrapRows,
} from "./pane-layout-model";

const simple = (): Row[] => [
  {
    slots: [
      { pane: "preview", size: 3 },
      { pane: "bitrate", size: 4 },
    ],
  },
  {
    slots: [
      { pane: "audio", size: 1 },
      { pane: null, size: 1 },
    ],
  },
];
describe("rows layout storage", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    localStorage.clear();
  });
  it("uses per-switcher and per-mode keys, never per-width or old grid keys", () => {
    expect(storageKey("one", "streamer")).not.toBe(storageKey("one", "operator"));
    expect(storageKey("one", "streamer")).not.toBe(storageKey("two", "streamer"));
    expect(storageKey("one", "streamer")).toBe("noalbs-live-rows:v1:one:streamer");
    localStorage.setItem(
      "noalbs-live-layout:v22:one:streamer",
      JSON.stringify({ grids: { 12: [] } }),
    );
    expect(read(storageKey("one", "streamer"), "streamer")).toEqual(defaults("streamer"));
  });
  it("reads one desktop layout independently of render width, with separate phone order", () => {
    const value = { desktop: simple(), phone: ["audio", "scenes"] };
    const key = storageKey("one", "operator");
    localStorage.setItem(key, JSON.stringify(value));
    for (const width of [560, 900, 1200, 1440, 1920, 2560]) {
      wrapRows(value.desktop, width, false);
      expect(read(key, "operator")).toEqual(value);
    }
  });
  it("accepts defaults, all-hidden layouts, empty slots and a row minimum height", () => {
    expect(valid(defaults("streamer"))).toBe(true);
    expect(valid(defaults("operator"))).toBe(true);
    expect(valid({ desktop: [], phone: [] })).toBe(true);
    expect(
      valid({ desktop: [{ slots: [{ pane: null, size: 2 }], minHeight: 250 }], phone: [] }),
    ).toBe(true);
  });
  it.each([
    null,
    {},
    { desktop: [], phone: ["unknown"] },
    { desktop: [], phone: ["audio", "audio"] },
    { desktop: [{ slots: [] }], phone: [] },
    { desktop: [{ slots: Array(5).fill({ pane: null, size: 1 }) }], phone: [] },
    {
      desktop: [
        {
          slots: [
            { pane: "audio", size: 1 },
            { pane: "audio", size: 1 },
          ],
        },
      ],
      phone: [],
    },
    { desktop: [{ slots: [{ pane: "unknown", size: 1 }] }], phone: [] },
    ...[0, -1, NaN, Infinity, "1", null].map((size) => ({
      desktop: [{ slots: [{ pane: "audio", size }] }],
      phone: [],
    })),
    ...[0, -1, NaN, Infinity, "100"].map((minHeight) => ({
      desktop: [{ slots: [{ pane: "audio", size: 1 }], minHeight }],
      phone: [],
    })),
    { desktop: [null], phone: [] },
    { desktop: [{ slots: [null] }], phone: [] },
  ])("rejects malformed layout %#", (value) => expect(valid(value)).toBe(false));
  it("falls back on invalid JSON, invalid data, missing and unavailable storage", () => {
    for (const value of [
      "oops",
      JSON.stringify({ grids: {} }),
      JSON.stringify({ desktop: [], phone: ["bad"] }),
    ]) {
      localStorage.setItem("key", value);
      expect(read("key", "operator")).toEqual(defaults("operator"));
    }
    expect(read("missing", "streamer")).toEqual(defaults("streamer"));
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    expect(read("key", "streamer")).toEqual(defaults("streamer"));
  });
});
describe("visual wrapping", () => {
  it("moves only trailing slots and preserves the saved rows and weights", () => {
    const rows = defaults("streamer").desktop;
    const original = structuredClone(rows);
    expect(wrapRows(rows, 1112, false).map((r) => r.slots.map((s) => s.slot.pane))).toEqual([
      ["preview", "on-air", "bitrate"],
      ["scenes", "audio", "servers"],
      ["sources", "obs", "belabox"],
      ["moblin"],
      ["activity"],
    ]);
    expect(wrapRows(rows, 1592, false).length).toBe(4);
    expect(rows).toEqual(original);
  });
  it("stacks below 700 in desktop order and handles empty layouts and rows", () => {
    const rows = simple();
    expect(wrapRows(rows, 699, false).map((r) => r.slots[0]?.slot.pane)).toEqual([
      "preview",
      "bitrate",
      "audio",
    ]);
    expect(wrapRows([], 1000, false)).toEqual([]);
    expect(wrapRows([{ slots: [{ pane: null, size: 1 }] }], 1000, false)).toEqual([]);
    expect(wrapRows([{ slots: [{ pane: null, size: 1 }] }], 1000, true)).toHaveLength(1);
  });
  it("omits unavailable panes and carries original row/slot indices through wrapping", () => {
    const rows = wrapRows(simple(), 700, true, (id) => id !== "preview");
    expect(rows[0]?.slots[0]?.index).toBe(1);
    expect(rows[0]?.source).toBe(0);
    expect(rows.flatMap((r) => r.slots).some((s) => s.slot.pane === "preview")).toBe(false);
  });
  it("renormalizes visual widths by weight without squeezing below content minima", () => {
    expect(slotWidths(simple()[0]?.slots ?? [], 716)).toEqual([300, 400]);
    const widths = slotWidths(
      [
        { pane: "preview", size: 100 },
        { pane: "bitrate", size: 1 },
      ],
      1016,
    );
    expect(widths).toEqual([700, 300]);
    expect(
      slotWidths(
        [
          { pane: "audio", size: 1e308 },
          { pane: "bitrate", size: 1e308 },
        ],
        1016,
      ),
    ).toEqual([500, 500]);
    expect(slotWidths([{ pane: "monitors", size: 1 }], 500)).toEqual([500]);
  });
});
describe("pair resizing", () => {
  it("keeps an unrelated free slot fixed when a constrained pair becomes free", () => {
    const slots: Row["slots"] = [
      { pane: "preview", size: 3 },
      { pane: "on-air", size: 0.52 },
      { pane: "bitrate", size: 5.48 },
    ];
    const widths = slotWidths(slots, 1592);
    const unit = Math.min(...slots.map((s, i) => (widths[i] ?? 0) / s.size));
    const sizes = resizePair([3, 0.52], [widths[0] ?? 0, widths[1] ?? 0], -40, [300, 220], unit);
    const next = slotWidths(
      slots.map((s, i) => (i < 2 ? { ...s, size: sizes[i] ?? s.size } : s)),
      1592,
    );
    expect(next[0]).toBeCloseTo((widths[0] ?? 0) - 40);
    expect(next[1]).toBeCloseTo((widths[1] ?? 0) + 40);
    expect(next[2]).toBeCloseTo(widths[2] ?? 0);
  });
  it("clamps both directions and conserves the pair weight", () => {
    expect(resizePair([1, 1], [400, 400], -1000, [300, 220])).toEqual([0.75, 1.25]);
    expect(resizePair([1, 1], [400, 400], 1000, [300, 220])).toEqual([1.45, 0.55]);
    expect(resizePair([3, 1], [600, 200], -200, [200, 200])).toEqual([2, 2]);
  });
  it("leaves an impossible or invalid request unchanged", () => {
    expect(resizePair([1e308, 1e308], [400, 400], 30, [300, 300])).toEqual([1e308, 1e308]);
    expect(resizePair([1e307, 1e307], [400, 400], 0, [300, 300])).toEqual([1e307, 1e307]);
    expect(resizePair([1, 1], [100, 100], 30, [300, 300])).toEqual([1, 1]);
    expect(resizePair([1, 1], [400, 400], NaN, [300, 300])).toEqual([1, 1]);
  });
});
describe("row edits", () => {
  it("adds 1–4 empty columns at a chosen position and removes empty slots/rows", () => {
    let rows = reduceRows([], { type: "add-row", at: 0, columns: 2 });
    expect(rows[0]?.slots).toHaveLength(2);
    rows = reduceRows(rows, { type: "remove-slot", row: 0, slot: 0 });
    expect(rows[0]?.slots).toHaveLength(1);
    expect(reduceRows(rows, { type: "remove-slot", row: 0, slot: 0 })).toEqual([]);
    expect(reduceRows(simple(), { type: "remove-row", row: 0 })).toEqual([simple()[1]]);
    expect(reduceRows([], { type: "add-row", at: 0, columns: 5 })).toEqual([]);
  });
  it("adds beside, places and hides, prevents duplicates and caps at four slots", () => {
    const original = simple();
    let rows = reduceRows(original, { type: "beside", row: 0, slot: 0 });
    expect(rows[0]?.slots[1]?.pane).toBeNull();
    rows = reduceRows(rows, { type: "place", row: 0, slot: 1, pane: "obs" });
    expect(rows[0]?.slots[1]?.pane).toBe("obs");
    rows = reduceRows(rows, { type: "hide", row: 0, slot: 1 });
    expect(rows[0]?.slots[1]?.pane).toBeNull();
    expect(original).toEqual(simple());
    expect(reduceRows(rows, { type: "place", row: 0, slot: 1, pane: "audio" })).toBe(rows);
    rows = reduceRows(rows, { type: "beside", row: 0, slot: 0 });
    expect(rows[0]?.slots).toHaveLength(4);
    expect(reduceRows(rows, { type: "beside", row: 0, slot: 0 })).toBe(rows);
  });
  it("swaps centres without changing slots or weights", () => {
    const rows = reduceRows(simple(), {
      type: "drop",
      source: { row: 0, slot: 0 },
      target: { kind: "center", row: 1, slot: 0 },
    });
    expect(rows[0]?.slots[0]).toEqual({ pane: "audio", size: 3 });
    expect(rows[1]?.slots[0]).toEqual({ pane: "preview", size: 1 });
  });
  it("places into an empty slot and removes the original slot", () => {
    const rows = reduceRows(simple(), {
      type: "drop",
      source: { row: 0, slot: 0 },
      target: { kind: "center", row: 1, slot: 1 },
    });
    expect(rows[0]?.slots.map((s) => s.pane)).toEqual(["bitrate"]);
    expect(rows[1]?.slots.map((s) => s.pane)).toEqual(["audio", "preview"]);
  });
  it.each(["left", "right"] as const)(
    "inserts on the %s edge and collapses an emptied source row",
    (kind) => {
      const initial: Row[] = [
        { slots: [{ pane: "preview", size: 1 }] },
        { slots: [{ pane: "audio", size: 1 }] },
      ];
      const rows = reduceRows(initial, {
        type: "drop",
        source: { row: 0, slot: 0 },
        target: { kind, row: 1, slot: 0 },
      });
      expect(rows).toHaveLength(1);
      expect(rows[0]?.slots.map((s) => s.pane)).toEqual(
        kind === "left" ? ["preview", "audio"] : ["audio", "preview"],
      );
    },
  );
  it("moves within a row without losing the destination index", () => {
    const rows = reduceRows(simple(), {
      type: "drop",
      source: { row: 0, slot: 0 },
      target: { kind: "right", row: 0, slot: 1 },
    });
    expect(rows[0]?.slots.map((s) => s.pane)).toEqual(["bitrate", "preview"]);
  });
  it("creates a one-slot row at a between-rows target, even before the source", () => {
    const rows = reduceRows(simple(), {
      type: "drop",
      source: { row: 1, slot: 0 },
      target: { kind: "row", row: 0 },
    });
    expect(rows[0]?.slots[0]?.pane).toBe("audio");
    expect(rows[2]?.slots[0]?.pane).toBeNull();
  });
  it("changes only a pair of weights or row minHeight, with auto reset", () => {
    const initial = simple();
    let rows = reduceRows(initial, { type: "sizes", row: 0, left: 0, right: 1, sizes: [2, 5] });
    expect(rows[1]).toEqual(initial[1]);
    rows = reduceRows(rows, { type: "height", row: 0, value: 400 });
    expect(rows[0]?.minHeight).toBe(400);
    rows = reduceRows(rows, { type: "height", row: 0 });
    expect(rows[0]?.minHeight).toBeUndefined();
  });
  it("ignores invalid targets, self drops, non-empty slot removal and full-row insertion", () => {
    const rows = simple();
    expect(
      reduceRows(rows, {
        type: "drop",
        source: { row: 0, slot: 0 },
        target: { kind: "center", row: 0, slot: 0 },
      }),
    ).toBe(rows);
    expect(
      reduceRows(rows, {
        type: "drop",
        source: { row: 9, slot: 0 },
        target: { kind: "row", row: 0 },
      }),
    ).toBe(rows);
    expect(reduceRows(rows, { type: "remove-slot", row: 0, slot: 0 })).toBe(rows);
    expect(reduceRows(rows, { type: "sizes", row: 0, left: 0, right: 1, sizes: [0, 1] })).toBe(
      rows,
    );
    const full = defaults("streamer").desktop;
    expect(
      reduceRows(full, {
        type: "drop",
        source: { row: 0, slot: 0 },
        target: { kind: "left", row: 2, slot: 0 },
      }),
    ).toBe(full);
  });
});
