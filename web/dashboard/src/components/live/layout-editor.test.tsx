import { i18n } from "@lingui/core";
import { I18nProvider } from "@lingui/react";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  ALL_DEVICES,
  type Availability,
  type Columns,
  layoutStorageKey,
  minSpan,
  PRESETS,
  TILE_IDS,
} from "./layout";
import { LiveLayoutEditor } from "./layout-editor";
import { LiveTile } from "./tile";
import { useLiveLayout } from "./use-live-layout";

let root: Root;
let host: HTMLDivElement;
let layout: ReturnType<typeof useLiveLayout>;
function Probe({
  desktop = true,
  cols = 12,
  availability = ALL_DEVICES,
}: {
  desktop?: boolean;
  cols?: Columns;
  availability?: Availability;
}) {
  layout = useLiveLayout("editor-user", "editor-switcher", cols, availability);
  return (
    <I18nProvider i18n={i18n}>
      <LiveLayoutEditor layout={layout} cols={cols} desktop={desktop} availability={availability}>
        {({ id }) => (
          <LiveTile title={id}>
            <button type="button">Content {id}</button>
          </LiveTile>
        )}
      </LiveLayoutEditor>
    </I18nProvider>
  );
}
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  localStorage.clear();
  vi.unstubAllGlobals();
});
function button(name: string) {
  const found = Array.from(document.querySelectorAll<HTMLButtonElement>("button")).find(
    (element) => element.getAttribute("aria-label") === name || element.textContent === name,
  );
  if (!found) throw new Error(`Missing button: ${name}`);
  return found;
}
const click = (name: string) => act(() => button(name).click());
const span = (id: string) =>
  Number(host.querySelector(`[data-panel="${id}"]`)?.getAttribute("data-span"));

it("toggles editing, makes content inert, and exits when leaving desktop", () => {
  act(() => root.render(<Probe />));
  expect(host.querySelector("[inert]")).toBeNull();
  click("Edit layout");
  expect(host.querySelectorAll('[data-panel="hero"] h2')).toHaveLength(1);
  expect(host.querySelector('[data-panel="hero"] h2')?.textContent).toBe("Stream");
  expect(host.querySelector('[data-panel="hero"] section header')).toBeNull();
  expect(host.querySelector('[data-panel="hero"] [inert]')?.getAttribute("aria-hidden")).toBe(
    "true",
  );
  expect(button("Move Stream").getAttribute("aria-describedby")).toBeTruthy();
  click("Done");
  expect(host.querySelectorAll('[data-panel="hero"] h2')).toHaveLength(1);
  expect(host.querySelector('[data-panel="hero"] section header')).not.toBeNull();
  expect(host.querySelector("[inert]")).toBeNull();
  expect(document.activeElement).toBe(button("Edit layout"));
  click("Edit layout");
  act(() => root.render(<Probe desktop={false} />));
  expect(host.textContent).not.toContain("Edit layout");
  expect(host.querySelector("[inert]")).toBeNull();
  act(() => root.render(<Probe />));
  expect(button("Edit layout")).toBeTruthy();
  expect(host.textContent).not.toContain("Reset to preset");
});

it("hides and adds panels immediately, excludes unavailable devices, and shows the all-shown empty state", () => {
  act(() => root.render(<Probe availability={{ belabox: false, moblin: false }} />));
  click("Edit layout");
  click("Hide Audio");
  expect(host.querySelector('[data-panel="audio"]')).toBeNull();
  expect(localStorage.getItem(layoutStorageKey("editor-user", "editor-switcher"))).not.toContain(
    '"id":"audio"',
  );
  click("Add panel");
  expect(document.querySelector('[data-slot="popover-content"]')?.textContent).not.toContain(
    "BELABOX",
  );
  click("Audio");
  expect(host.querySelector('[data-panel="audio"]')).not.toBeNull();
  act(() => layout.updateTiles(TILE_IDS.map((id) => ({ id, span: minSpan(id) }))));
  click("Add panel");
  expect(document.body.textContent).toContain("All panels are shown.");
});

it("clamps minus/plus and keyboard resizing and keeps column customizations independent", () => {
  act(() => root.render(<Probe />));
  click("Edit layout");
  for (let i = 0; i < 20; i++) click("Make Stream narrower");
  expect(span("hero")).toBe(6);
  expect(button("Make Stream narrower").disabled).toBe(true);
  for (let i = 0; i < 20; i++) click("Make Stream wider");
  expect(span("hero")).toBe(12);
  expect(button("Make Stream wider").disabled).toBe(true);
  act(() => root.render(<Probe cols={16} />));
  expect(button("Done")).toBeTruthy();
  expect(span("hero")).toBe(PRESETS[16].default.find((tile) => tile.id === "hero")?.span);
  const slider = host.querySelector('[role="slider"][aria-label="Resize Stream"]');
  act(() => slider?.dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true })));
  expect(span("hero")).toBe(16);
  act(() => slider?.dispatchEvent(new KeyboardEvent("keydown", { key: "Home", bubbles: true })));
  expect(span("hero")).toBe(6);
  act(() => root.render(<Probe />));
  expect(span("hero")).toBe(12);
});

it("preset selection and reset clear customizations for both grids and retain the selected preset", () => {
  act(() => root.render(<Probe />));
  click("Edit layout");
  click("Hide Audio");
  act(() => root.render(<Probe cols={16} />));
  click("Hide Audio");
  const select = host.querySelector<HTMLSelectElement>('select[aria-label="Layout preset"]');
  act(() => {
    if (!select) throw new Error("Missing preset select");
    select.value = "compact";
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
  expect(layout.layout.grids).toEqual({});
  expect(layout.tiles).toEqual(PRESETS[16].compact);
  act(() => root.render(<Probe />));
  expect(layout.tiles).toEqual(PRESETS[12].compact);
  click("Make Stream wider");
  click("Reset to preset");
  expect(layout.layout.grids).toEqual({});
  expect(layout.preset).toBe("compact");
  expect(layout.tiles).toEqual(PRESETS[12].compact);
});
