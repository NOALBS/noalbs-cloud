import { i18n } from "@lingui/core";
import { I18nProvider } from "@lingui/react";
import { type AnchorHTMLAttributes, act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { LiveList } from "./live-list";
import type { OperatorData } from "./model";
import { PINS_KEY } from "./pins";

const media = vi.hoisted(() => ({ desktop: true, phone: false }));
vi.mock("@/hooks/use-media-query", () => ({
  useMediaQuery: (query: string) =>
    query.includes("64rem") ? media.desktop : query.includes("max-width") ? media.phone : false,
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
vi.mock("./phone-sheet", () => ({
  PhoneSheet: ({ onClose }: { onClose: () => void }) => (
    <div role="dialog" aria-label="Phone actions">
      <button type="button" onClick={onClose}>
        Close
      </button>
    </div>
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
  media.phone = false;
  localStorage.clear();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
  vi.useRealTimers();
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
  it("forces phone rows regardless of saved columns, with counts and expandable search", async () => {
    media.phone = true;
    media.desktop = false;
    localStorage.setItem("noalbs-operator-view", "side-by-side");
    await render();
    expect(container.querySelector('[aria-label="List view"]')).toBeNull();
    expect(container.textContent).toContain("All 1");
    expect(container.textContent).toContain("Streaming 1");
    expect(container.querySelector("input")).toBeNull();
    await act(async () =>
      container.querySelector<HTMLButtonElement>('[aria-label="Find a switcher"]')?.click(),
    );
    expect(container.querySelector("input")).not.toBeNull();
    expect(localStorage.getItem("noalbs-operator-view")).toBe("side-by-side");
  });
  it("opens phone controls after 450ms and suppresses the hold's navigation click", async () => {
    vi.useFakeTimers();
    media.phone = true;
    media.desktop = false;
    await render();
    const link = container.querySelector("a");
    await act(async () => {
      link?.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 }));
      vi.advanceTimersByTime(449);
    });
    expect(container.querySelector('[role="dialog"]')).toBeNull();
    await act(async () => vi.advanceTimersByTime(1));
    expect(container.querySelector('[role="dialog"]')).not.toBeNull();
    const click = new MouseEvent("click", { bubbles: true, cancelable: true, detail: 1 });
    await act(async () => {
      link?.dispatchEvent(new MouseEvent("pointerup", { bubbles: true }));
      link?.dispatchEvent(click);
    });
    expect(click.defaultPrevented).toBe(true);
  });
  it("cancels a hold on movement, scrolling and pointer cancellation; overflow remains accessible", async () => {
    vi.useFakeTimers();
    media.phone = true;
    media.desktop = false;
    await render();
    const link = container.querySelector("a");
    for (const kind of ["pointermove", "scroll", "pointercancel"]) {
      await act(async () => {
        link?.dispatchEvent(
          new MouseEvent("pointerdown", { bubbles: true, button: 0, clientX: 10, clientY: 10 }),
        );
        if (kind === "scroll") window.dispatchEvent(new Event("scroll"));
        else link?.dispatchEvent(new MouseEvent(kind, { bubbles: true, clientX: 22, clientY: 10 }));
        vi.advanceTimersByTime(500);
      });
      expect(container.querySelector('[role="dialog"]')).toBeNull();
    }
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Actions for Studio / Main"]')
        ?.click(),
    );
    expect(container.querySelector('[role="dialog"]')).not.toBeNull();
  });
  it("supports context-menu actions without navigating", async () => {
    media.phone = true;
    media.desktop = false;
    await render();
    const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
    await act(async () => {
      container.querySelector("a")?.dispatchEvent(event);
    });
    expect(event.defaultPrevented).toBe(true);
    expect(container.querySelector('[role="dialog"]')).not.toBeNull();
    await act(async () =>
      container.querySelector<HTMLButtonElement>('[role="dialog"] button')?.click(),
    );
    expect(container.querySelector('[role="dialog"]')).toBeNull();
  });
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
