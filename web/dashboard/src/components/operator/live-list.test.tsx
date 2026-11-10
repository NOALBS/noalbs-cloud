import { i18n } from "@lingui/core";
import { I18nProvider } from "@lingui/react";
import { type AnchorHTMLAttributes, act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { LiveList } from "./live-list";
import type { OperatorData } from "./model";
import { PINS_KEY } from "./pins";

const media = vi.hoisted(() => ({ desktop: true }));
vi.mock("@/hooks/use-media-query", () => ({
  useMediaQuery: (query: string) => (query.includes("64rem") ? media.desktop : false),
}));
vi.mock("@tanstack/react-router", () => ({
  Link: ({
    children,
    params: _params,
    to: _to,
    ...props
  }: AnchorHTMLAttributes<HTMLAnchorElement> & { params: unknown; to: string }) => (
    <a href="#live" {...props}>
      {children}
    </a>
  ),
}));
vi.mock("./compact-live", () => ({
  CompactLive: ({ onClose, onPin }: { onClose: () => void; onPin: () => void }) => (
    <section aria-label="Compact live">
      <button type="button" onClick={onClose}>
        Close
      </button>
      <button type="button" onClick={onPin}>
        Pin
      </button>
    </section>
  ),
}));

const data: OperatorData = {
  items: [
    {
      switcher_id: "one",
      switcher_name: "Main",
      workspace_id: "w",
      workspace_name: "Studio",
      workspace_slug: "studio",
      connected: true,
      obs_connected: true,
      can_control: false,
      streaming: true,
      recording: false,
      paused: false,
      state: "live",
    },
  ],
  counts: { streaming: 1, attention: 0, connected: 1, recording: 0 },
  total: 1,
  perPage: 50,
  offlineTotal: 0,
};
let root: Root;
let container: HTMLDivElement;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  media.desktop = true;
  localStorage.clear();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});
async function render(search = {}, onOpen = vi.fn()) {
  await act(async () =>
    root.render(
      <I18nProvider i18n={i18n}>
        <LiveList
          data={data}
          updatedAt={0}
          pending={false}
          scope="workspaces"
          search={search}
          onSearch={() => {}}
          onShow={() => {}}
          onOpen={onOpen}
        />
      </I18nProvider>,
    ),
  );
  return onOpen;
}
describe("operator selection and saved layouts", () => {
  it("opens desktop links as panels and preserves modified-link navigation", async () => {
    const onOpen = await render();
    const link = container.querySelector("a");
    await act(async () => {
      link?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, button: 0 }));
    });
    expect(onOpen).toHaveBeenCalledWith({ workspace: "studio", switcher: "one" });
    onOpen.mockClear();
    await act(async () => {
      link?.dispatchEvent(
        new MouseEvent("click", { bubbles: true, cancelable: true, button: 0, ctrlKey: true }),
      );
    });
    expect(onOpen).not.toHaveBeenCalled();
  });
  it("leaves phone links as real navigation", async () => {
    media.desktop = false;
    const onOpen = await render();
    const event = new MouseEvent("click", { bubbles: true, cancelable: true, button: 0 });
    await act(async () => {
      container.querySelector("a")?.dispatchEvent(event);
    });
    expect(event.defaultPrevented).toBe(false);
    expect(onOpen).not.toHaveBeenCalled();
  });
  it("restores URL selection, highlights its link and closes with Escape", async () => {
    const onOpen = await render({ open: "one", open_workspace: "studio" });
    expect(container.querySelector("aside")).not.toBeNull();
    expect(container.querySelector('a[aria-current="true"]')).not.toBeNull();
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    expect(onOpen).toHaveBeenCalledWith(undefined);
  });
  it("shows empty columns, persists picker pins and can unpin them", async () => {
    localStorage.setItem("noalbs-operator-view", "side-by-side");
    await render();
    expect(container.textContent).toContain("Keep up to four switchers side by side");
    const picker = container.querySelector("select");
    await act(async () => {
      if (picker) {
        picker.value = "one";
        picker.dispatchEvent(new Event("change", { bubbles: true }));
      }
    });
    expect(JSON.parse(localStorage.getItem(PINS_KEY) ?? "[]")).toEqual([
      { workspace: "studio", switcher: "one" },
    ]);
    await act(async () => {
      container.querySelector<HTMLButtonElement>('[aria-label="Compact live"] button')?.click();
    });
    expect(JSON.parse(localStorage.getItem(PINS_KEY) ?? "[]")).toEqual([]);
  });
});
