import { i18n } from "@lingui/core";
import { I18nProvider } from "@lingui/react";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { previewOn, usePreviewSettings } from "@/lib/obs-preview";
import { PreviewControls } from "./obs-preview";

let host: HTMLDivElement;
let root: Root;

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
});
