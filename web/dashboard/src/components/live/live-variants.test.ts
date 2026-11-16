import { describe, expect, it } from "vitest";
import { families, paneFamily, parseVariant } from "./live-variants";
import { modeSaved, operatorColumns, storageKey } from "./pane-layout";

describe("Live prototype variants and mode isolation", () => {
  it("defaults invalid or empty variants to the site Cards design", () => {
    for (const value of [null, "", "other", "A"]) expect(parseVariant(value)).toBe("a");
    expect(parseVariant("b")).toBe("b");
    expect(parseVariant("c")).toBe("c");
  });
  it("maps independent mixable pane families", () => {
    expect(paneFamily("controls")).toBe("actions");
    expect(paneFamily("program-preview")).toBe("monitors");
    expect(paneFamily("preview")).toBe("monitors");
    expect(paneFamily("status")).toBeNull();
    expect(families).toHaveLength(5);
  });
  it("uses one operator selection and preserves existing Streamer storage", () => {
    expect(storageKey("one", false, true, "a")).toBe(storageKey("one", false, true, "c"));
    expect(storageKey("one", false, true, "a")).toContain(":v12:");
    expect(storageKey("one", false, false, "a")).toBe(
      "noalbs-live-layout:v11:one:desktop:streamer:a",
    );
    const research = modeSaved(false, true, "a");
    expect(research).toEqual(modeSaved(false, true, "b"));
    for (const cols of [4, 5]) {
      const layout = research.layouts[cols] ?? [];
      expect(
        layout
          .filter((p) => ["sources", "audio", "belabox", "moblin"].includes(p.i))
          .map((p) => [p.x, p.y]),
      ).toEqual([
        [0, 7],
        [1, 7],
        [2, 7],
        [3, 7],
      ]);
      expect(layout.find((p) => p.i === "auto-switch")).toMatchObject({ x: 4, y: 3 });
      expect(layout.find((p) => p.i === "scenes")).toMatchObject({ x: 0, w: 4 });
    }
  });
  it("separates layouts by device, switcher, mode and arrangement", () => {
    const keys = [
      storageKey("one", false, false, "a"),
      storageKey("one", false, true, "a"),
      storageKey("one", false, false, "b"),
      storageKey("one", true, false, "a"),
      storageKey("two", false, false, "a"),
    ];
    expect(new Set(keys).size).toBe(keys.length);
  });
  it("never includes operator-only monitors in Streamer or phone defaults", () => {
    for (const variant of ["a", "b", "c"] as const) {
      const normal = modeSaved(false, false, variant);
      for (const ids of Object.values(normal.visibleByCols ?? {}))
        expect(ids).not.toContain("program-preview");
      expect(modeSaved(true, true, variant).visible).not.toContain("program-preview");
      expect(modeSaved(false, true, variant).visible).toContain("program-preview");
    }
  });
  it("fits each requested breakpoint without overlapping default panes", () => {
    for (const variant of ["a", "b", "c"] as const)
      for (const mode of [false, true])
        for (const cols of [3, 4, 5]) {
          const layout = modeSaved(false, mode, variant).layouts[cols] ?? [];
          expect(layout.length).toBeGreaterThan(0);
          for (const item of layout) {
            expect(item.x + item.w).toBeLessThanOrEqual(mode ? operatorColumns(cols) : cols);
            expect(item.y + item.h).toBeLessThanOrEqual(12);
          }
          for (let y = 0; y < 12; y++)
            for (let x = 0; x < (mode ? operatorColumns(cols) : cols); x++)
              expect(
                layout.filter((p) => x >= p.x && x < p.x + p.w && y >= p.y && y < p.y + p.h).length,
              ).toBeLessThanOrEqual(1);
        }
  });
});
