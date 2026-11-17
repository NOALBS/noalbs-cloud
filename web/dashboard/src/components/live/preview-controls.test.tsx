import { i18n } from "@lingui/core";
import { I18nProvider } from "@lingui/react";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { type PreviewSettings, previewOn, usePreviewSettings } from "@/lib/obs-preview";
import { PreviewControls } from "./obs-preview";

let host: HTMLDivElement;
let root: Root;
const updates: ((patch: Partial<PreviewSettings>) => void)[] = [];

function Reader({ index }: { index: number }) {
  const [settings, update] = usePreviewSettings();
  updates[index] = update;
  return <output>{JSON.stringify(settings)}</output>;
}

function Subject({ info }: { info?: string }) {
  const [settings, update] = usePreviewSettings();
  return (
    <PreviewControls settings={settings} update={update} on={previewOn(settings, true)}>
      {info}
    </PreviewControls>
  );
}

function render(info?: string) {
  act(() =>
    root.render(
      <I18nProvider i18n={i18n}>
        <Subject info={info} />
      </I18nProvider>,
    ),
  );
}

function click(label: string) {
  const button = host.querySelector<HTMLButtonElement>(`[aria-label="${label}"]`);
  if (!button) throw new Error(`Missing ${label}`);
  act(() => button.click());
}

describe("shared preview controls", () => {
  beforeEach(() => {
    (
      globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    localStorage.clear();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    vi.restoreAllMocks();
  });

  it("opens Rate, Size and Quality with space above the divider and around the footer", () => {
    render("12 KB per frame · about 99 kbps");
    expect(host.querySelector('[aria-label="Rate"]')).toBeNull();
    click("Preview settings");
    for (const label of ["Rate", "Size", "Quality"]) {
      expect(host.querySelector(`[aria-label="${label}"]`)).not.toBeNull();
    }
    const settings = host.querySelector('[aria-label="Rate"]')?.closest(".grid");
    expect(settings?.classList.contains("mt-2")).toBe(true);
    expect(settings?.previousElementSibling?.classList.contains("py-2")).toBe(true);
    expect(host.textContent).toContain("12 KB per frame · about 99 kbps");
  });

  it("works without bandwidth text and preserves settings across remounts", () => {
    localStorage.setItem(
      "noalbs-obs-preview",
      JSON.stringify({ enabled: false, fps: 2, width: 960, quality: 80 }),
    );
    render();
    expect(host.querySelector('[aria-label="Preview settings"]')).toBeNull();
    click("Show the OBS preview");
    expect(JSON.parse(localStorage.getItem("noalbs-obs-preview") ?? "{}")).toEqual({
      enabled: true,
      fps: 2,
      width: 960,
      quality: 80,
    });
    act(() => root.unmount());
    root = createRoot(host);
    render();
    expect(
      host.querySelector('[aria-label="Show the OBS preview"]')?.getAttribute("aria-checked"),
    ).toBe("true");
    click("Preview settings");
    expect(host.querySelectorAll('[role="combobox"]')).toHaveLength(3);
    click("Show the OBS preview");
    expect(host.querySelector('[aria-label="Rate"]')).toBeNull();
  });

  it("keeps the toggle usable when browser storage is unavailable", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("unavailable");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("unavailable");
    });
    render();
    click("Show the OBS preview");
    expect(
      host.querySelector('[aria-label="Show the OBS preview"]')?.getAttribute("aria-checked"),
    ).toBe("false");
    click("Show the OBS preview");
    expect(host.querySelector('[aria-label="Preview settings"]')).not.toBeNull();
  });

  it("synchronizes mounted instances immediately, including consecutive patches", () => {
    act(() =>
      root.render(
        <>
          <Reader index={0} />
          <Reader index={1} />
        </>,
      ),
    );
    const first = updates[0];
    const second = updates[1];
    if (!first || !second) throw new Error("Missing preview settings readers");
    act(() => {
      first({ enabled: false, fps: 5 });
      second({ width: 960, quality: 80 });
    });
    const expected = { enabled: false, fps: 5, width: 960, quality: 80 };
    for (const output of host.querySelectorAll("output"))
      expect(JSON.parse(output.textContent ?? "{}")).toEqual(expected);
    expect(JSON.parse(localStorage.getItem("noalbs-obs-preview") ?? "{}")).toEqual(expected);
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("unavailable");
    });
    act(() => second({ enabled: true }));
    for (const output of host.querySelectorAll("output"))
      expect(JSON.parse(output.textContent ?? "{}")).toEqual({ ...expected, enabled: true });
  });

  it("follows storage changes from other tabs and resets after storage is cleared", () => {
    render();
    localStorage.setItem(
      "noalbs-obs-preview",
      JSON.stringify({ enabled: false, fps: 2, width: 960, quality: 80 }),
    );
    act(() => window.dispatchEvent(new StorageEvent("storage", { key: "unrelated" })));
    expect(host.querySelector('[role="switch"]')?.getAttribute("aria-checked")).toBe("true");
    act(() => window.dispatchEvent(new StorageEvent("storage", { key: "noalbs-obs-preview" })));
    expect(host.querySelector('[role="switch"]')?.getAttribute("aria-checked")).toBe("false");
    localStorage.clear();
    act(() => window.dispatchEvent(new StorageEvent("storage", { key: null })));
    expect(host.querySelector('[role="switch"]')?.getAttribute("aria-checked")).toBe("true");
  });

  it("places controls in a custom toolbar and opens settings below it", () => {
    act(() =>
      root.render(
        <I18nProvider i18n={i18n}>
          <PreviewControls
            settings={{ enabled: true, fps: 1, width: 640, quality: 60 }}
            update={() => {}}
            on
            renderToolbar={(controls) => (
              <div data-toolbar>
                <button type="button">Take</button>
                {controls}
              </div>
            )}
          />
        </I18nProvider>,
      ),
    );
    const toolbar = host.querySelector("[data-toolbar]");
    expect(toolbar?.querySelector('[aria-label="Preview settings"]')).not.toBeNull();
    expect(toolbar?.querySelector('[role="switch"]')).not.toBeNull();
    click("Preview settings");
    expect(toolbar?.nextElementSibling?.querySelector('[aria-label="Rate"]')).not.toBeNull();
    expect(toolbar?.querySelector('[aria-label="Rate"]')).toBeNull();
  });
});
