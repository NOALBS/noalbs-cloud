import { i18n } from "@lingui/core";
import { I18nProvider } from "@lingui/react";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SceneGrid } from "./scene-grid";

const snapshot = vi.hoisted(() => ({
  data: undefined as { scenes: { name: string; uuid: string }[] } | undefined,
  isPending: false,
  isError: false,
  error: new Error("Could not load scenes"),
  refetch: vi.fn(),
}));
vi.mock("@tanstack/react-query", async (original) => ({
  ...(await original<typeof import("@tanstack/react-query")>()),
  useQuery: () => snapshot,
  useQueryClient: () => ({ invalidateQueries: vi.fn() }),
  useMutation: () => ({ isPending: false, mutate: vi.fn() }),
}));

let host: HTMLDivElement;
let root: Root;
const render = (count: number, connected = true, hotkeys = true) => {
  snapshot.data = {
    scenes: Array.from({ length: count }, (_, i) => ({ uuid: `scene-${i}`, name: `Scene ${i}` })),
  };
  act(() =>
    root.render(
      <I18nProvider i18n={i18n}>
        <SceneGrid
          path={{ workspace_id: "workspace", switcher_id: "switcher" }}
          scenes={{
            live: null,
            low: null,
            offline: null,
            starting: null,
            ending: null,
            privacy: null,
          }}
          currentUuid="scene-49"
          lastKnownUuid="scene-49"
          previewUuid="scene-48"
          streaming
          obsConnected={connected}
          canControl
          canManage={false}
          hotkeys={hotkeys}
          onPreviewSelect={vi.fn()}
        />
      </I18nProvider>,
    ),
  );
};

describe("large scene collections", () => {
  beforeEach(() => {
    (
      globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    localStorage.clear();
    snapshot.isError = false;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
  });
  it("keeps small collections unchanged and shows search only above eight scenes", () => {
    render(6);
    expect(host.querySelector('[type="search"]')).toBeNull();
    expect(host.querySelector(".live-collection-scroll")).toBeNull();
    render(8);
    expect(host.querySelector('[type="search"]')).toBeNull();
    render(9);
    expect(host.querySelector('[type="search"]')).not.toBeNull();
  });
  it("bounds large collections, pins both selections, and limits stable number hints to nine", () => {
    render(50);
    expect(host.querySelectorAll("[data-scene-view] button")).toHaveLength(50);
    expect(host.querySelectorAll("[data-scene-view] kbd")).toHaveLength(9);
    const strip = host.querySelector('[aria-label="Scene selections"]');
    expect(strip?.textContent).toContain("On airScene 49");
    expect(strip?.textContent).toContain("PreviewScene 48");
    expect(strip?.closest(".live-collection-scroll")).toBeNull();
    expect(host.querySelector('[title="Scene 49"]')).not.toBeNull();
    expect(
      host.querySelector("[data-scene-view]")?.classList.contains("live-collection-scroll"),
    ).toBe(true);
  });
  it("labels pinned selections as last known while disconnected", () => {
    render(50, false);
    const strip = host.querySelector('[aria-label="Scene selections"]');
    expect(strip?.textContent).toContain("Last knownScene 49");
    expect(strip?.textContent).toContain("Preview · last knownScene 48");
    expect(strip?.textContent).not.toContain("On air");
  });
  it("omits number hints and the slash prompt when operator hotkeys are inactive", () => {
    render(50, true, false);
    expect(host.querySelectorAll("kbd")).toHaveLength(0);
    expect(host.querySelector("input")?.placeholder).toBe("Search scenes");
    expect(host.textContent).not.toContain("1–9: first nine scenes");
  });
  it("keeps an empty collection usable without search or a fixed height", () => {
    render(0);
    expect(host.querySelector('[type="search"]')).toBeNull();
    expect(host.querySelector(".live-collection-scroll")).toBeNull();
    expect(host.querySelectorAll("[data-scene-view] button")).toHaveLength(0);
  });
  it("retains the load-error message and retry action", () => {
    snapshot.isError = true;
    render(0);
    expect(host.textContent).toContain("Couldn’t load scenes.");
    expect(host.textContent).toContain("Retry");
  });
});
