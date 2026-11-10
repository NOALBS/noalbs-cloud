import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { ALL_DEVICES, type Columns, layoutStorageKey, PRESETS } from "./layout";
import { useLiveLayout } from "./use-live-layout";

let result: ReturnType<typeof useLiveLayout>;
function Probe({
  user = "u",
  switcher = "s",
  cols = 12,
}: {
  user?: string;
  switcher?: string;
  cols?: Columns;
}) {
  result = useLiveLayout(user, switcher, cols, ALL_DEVICES);
  return null;
}

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

it("persists edits, keeps grids independent, resets and changes user/switcher scope", () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const root = createRoot(document.createElement("div"));
  act(() => root.render(<Probe />));
  act(() => result.updateTiles([]));
  expect(result?.tiles).toEqual([]);
  expect(JSON.parse(localStorage.getItem(layoutStorageKey("u", "s")) ?? "null").grids[12]).toEqual(
    [],
  );
  act(() => root.render(<Probe cols={16} />));
  expect(result?.tiles).toEqual(PRESETS[16].default);
  act(() => result.setPreset("compact"));
  expect(result?.tiles).toEqual(PRESETS[16].compact);
  expect(result.layout.grids).toEqual({});
  act(() => result.updateTiles([{ id: "hero", span: 99 }]));
  expect(result?.tiles).toEqual([{ id: "hero", span: 16 }]);
  act(() => root.render(<Probe />));
  act(() => result.updateTiles([]));
  act(() => root.render(<Probe cols={16} />));
  act(() => result.reset());
  expect(result?.tiles).toEqual(PRESETS[16].compact);
  expect(result.layout.grids).toEqual({});
  act(() => root.render(<Probe />));
  expect(result?.tiles).toEqual(PRESETS[12].compact);
  act(() => result.updateTiles([]));
  act(() =>
    result.updateSections((sections) => sections.map((section) => ({ ...section, hidden: true }))),
  );
  expect(result?.sections.every((section) => section.hidden)).toBe(true);
  act(() => root.render(<Probe user="other" switcher="other" />));
  expect(result?.tiles).toEqual(PRESETS[12].default);
  expect(result?.sections.every((section) => !section.hidden)).toBe(true);
  act(() => root.render(<Probe />));
  expect(result?.tiles).toEqual([]);
  expect(result?.sections.every((section) => section.hidden)).toBe(true);
  act(() => root.unmount());
  vi.unstubAllGlobals();
});

it("keeps edits in memory when storage throws", () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("unavailable");
  });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("unavailable");
  });
  const root = createRoot(document.createElement("div"));
  act(() => root.render(<Probe />));
  expect(result?.tiles).toEqual(PRESETS[12].default);
  act(() => result.updateTiles([]));
  expect(result?.tiles).toEqual([]);
  act(() => root.unmount());
  vi.unstubAllGlobals();
});
