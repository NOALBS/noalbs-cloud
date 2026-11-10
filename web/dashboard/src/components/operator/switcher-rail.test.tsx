import { i18n } from "@lingui/core";
import { I18nProvider } from "@lingui/react";
import { type AnchorHTMLAttributes, act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Workspace } from "@/api/types";
import { AttentionCount } from "./attention-count";
import type { OperatorItem } from "./model";
import { RAIL_KEY, SwitcherRail } from "./switcher-rail";

const fixture = vi.hoisted(() => ({
  desktop: true,
  total: 2,
  attention: 1,
  items: [] as OperatorItem[],
}));
vi.mock("@/hooks/use-media-query", () => ({ useMediaQuery: () => fixture.desktop }));
vi.mock("./use-visible-switchers", () => ({
  useVisibleSwitchers: () => ({
    total: fixture.total,
    attention: fixture.attention,
    offline: 3,
    groups: { attention: fixture.items, streaming: [], idle: [] },
  }),
}));
vi.mock("@tanstack/react-router", () => ({
  Link: ({
    children,
    to,
    params,
    search,
    ...props
  }: AnchorHTMLAttributes<HTMLAnchorElement> & {
    to: string;
    params?: { workspace: string; switcher: string };
    search?: { show: string };
  }) => (
    <a
      href={
        params
          ? `/${params.workspace}/s/${params.switcher}/live`
          : to + (search ? `?show=${search.show}` : "")
      }
      {...props}
    >
      {children}
    </a>
  ),
}));
const item: OperatorItem = {
  switcher_id: "s",
  switcher_name: "Main",
  workspace_id: "w",
  workspace_name: "Studio",
  workspace_slug: "studio",
  connected: true,
  can_control: true,
  obs_connected: true,
  streaming: true,
  recording: false,
  paused: false,
  state: "offline",
  attention: "no_feed",
  attention_since: new Date(Date.now() - 102000).toISOString(),
};
let root: Root;
let container: HTMLDivElement;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  fixture.desktop = true;
  fixture.total = 2;
  fixture.attention = 1;
  fixture.items = [item];
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
async function render(currentId: string | undefined = "s") {
  const props = {
    workspace: { id: "w", name: "Studio", url_slug: "studio" } as Workspace,
    actor: { role: "owner", staff: false } as const,
  };
  await act(async () =>
    root.render(
      <I18nProvider i18n={i18n}>
        <AttentionCount {...props} />
        <SwitcherRail {...props} currentId={currentId}>
          <main>Live content</main>
        </SwitcherRail>
      </I18nProvider>,
    ),
  );
}
describe("operator shell navigation", () => {
  it("renders the attention destination, grouped rail and selected Live link", async () => {
    await render();
    expect(
      container.querySelector('a[href="/workspaces/live?show=attention"]')?.textContent,
    ).toContain("1 needs attention");
    expect(container.querySelector('a[aria-current="page"]')?.getAttribute("href")).toBe(
      "/studio/s/s/live",
    );
    expect(container.textContent).toContain("Needs attention");
    expect(container.textContent).toContain("3 offline");
  });
  it("collapses to labelled dots, persists the state and restores it on a new mount", async () => {
    await render();
    await act(async () =>
      container.querySelector<HTMLButtonElement>('[aria-label="Collapse switcher rail"]')?.click(),
    );
    expect(localStorage.getItem(RAIL_KEY)).toBe("true");
    expect(container.querySelector("aside")?.getAttribute("data-collapsed")).toBe("true");
    expect(container.querySelector('a[aria-current="page"]')?.getAttribute("aria-label")).toContain(
      "Studio / Main",
    );
    await act(async () => root.unmount());
    root = createRoot(container);
    await render();
    expect(container.querySelector("aside")?.getAttribute("data-collapsed")).toBe("true");
  });
  it("leaves exactly the existing main DOM for a single switcher", async () => {
    fixture.total = 1;
    await render();
    expect(container.innerHTML).toBe("<main>Live content</main>");
  });
  it("skips phones and rail-less pages while keeping the desktop attention count", async () => {
    await render("");
    expect(container.querySelector("aside")).toBeNull();
    expect(container.querySelector('a[href*="show=attention"]')).not.toBeNull();
    fixture.desktop = false;
    await render();
    expect(container.innerHTML).toBe("<main>Live content</main>");
  });
  it("omits an empty attention count", async () => {
    fixture.attention = 0;
    await render();
    expect(container.querySelector('a[href*="show=attention"]')).toBeNull();
  });
});
