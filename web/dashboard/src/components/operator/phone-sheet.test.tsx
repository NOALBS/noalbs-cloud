import { i18n } from "@lingui/core";
import { I18nProvider } from "@lingui/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { type AnchorHTMLAttributes, act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "@/api/client";
import type { OperatorItem } from "./model";
import { PhoneSheet } from "./phone-sheet";

const fixture = vi.hoisted(() => ({
  scenes: [{ name: "BRB", uuid: "scene" }],
  error: false,
  connection: "open",
  fix: vi.fn(),
  stream: vi.fn(),
}));
vi.mock("@tanstack/react-router", () => ({
  Link: ({
    children,
    to: _to,
    params: _params,
    ...props
  }: AnchorHTMLAttributes<HTMLAnchorElement> & { to: string; params: unknown }) => (
    <a href="#live" {...props}>
      {children}
    </a>
  ),
}));
vi.mock("@/api/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/api/client")>()),
  api: { POST: vi.fn(async () => ({})) },
  unwrap: async (value: unknown) => value,
}));
vi.mock("@/api/queries", () => ({
  queryKeys: { obsScenes: () => ["scenes"] },
  obsScenesQuery: () => ({
    queryKey: ["scenes"],
    queryFn: async () => {
      if (fixture.error) throw new Error("Scenes unavailable");
      return { scenes: fixture.scenes };
    },
  }),
}));
vi.mock("@/live/use-switcher-live", () => ({
  useSwitcherLive: () => ({
    connection: fixture.connection,
    status: {
      updated_at: new Date().toISOString(),
      obs_connected: true,
      streaming: true,
      state: "offline",
      paused: false,
    },
  }),
}));
vi.mock("@/components/live/quick-actions", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/components/live/quick-actions")>()),
  useObsControls: () => ({
    fix: { mutate: fixture.fix, isPending: false },
    stream: { mutate: fixture.stream, isPending: false },
  }),
}));

const item: OperatorItem = {
  switcher_id: "s",
  switcher_name: "Main",
  workspace_id: "w",
  workspace_name: "Studio",
  workspace_slug: "studio",
  role: "operator",
  connected: true,
  can_control: true,
  obs_connected: true,
  streaming: true,
  recording: false,
  paused: false,
  state: "offline",
  attention: "no_feed",
};
let root: Root;
let container: HTMLDivElement;
let client: QueryClient;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  fixture.scenes = [{ name: "BRB", uuid: "scene" }];
  fixture.error = false;
  fixture.connection = "open";
  fixture.fix.mockClear();
  fixture.stream.mockClear();
  vi.mocked(api.POST).mockClear();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
});
afterEach(async () => {
  await act(async () => root.unmount());
  client.clear();
  container.remove();
  vi.unstubAllGlobals();
});
async function render(patch: Partial<OperatorItem> = {}) {
  await act(async () =>
    root.render(
      <I18nProvider i18n={i18n}>
        <QueryClientProvider client={client}>
          <PhoneSheet item={{ ...item, ...patch }} multi onClose={() => {}} />
        </QueryClientProvider>
      </I18nProvider>,
    ),
  );
}
function button(label: string) {
  return Array.from(document.querySelectorAll("button")).find(
    (button) => button.textContent?.trim() === label,
  );
}
async function click(label: string) {
  await act(async () => {
    button(label)?.click();
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
}
describe("phone live action sheet", () => {
  it("fixes feed, switches scenes and keeps stop behind the reused confirmation", async () => {
    await render();
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain("Studio / Main");
    await click("Fix feed");
    expect(fixture.fix).toHaveBeenCalledOnce();
    await click("Switch scene");
    expect(button("BRB")).toBeDefined();
    await click("BRB");
    expect(api.POST).toHaveBeenCalledWith(
      "/workspaces/{workspace_id}/switchers/{switcher_id}/obs/scene",
      { params: { path: { workspace_id: "w", switcher_id: "s" } }, body: { scene_uuid: "scene" } },
    );
    await click("Stop stream");
    expect(document.querySelector('[role="alertdialog"]')?.textContent).toContain(
      "Stop the stream?",
    );
    expect(fixture.stream).not.toHaveBeenCalled();
    await click("Keep streaming");
    expect(fixture.stream).not.toHaveBeenCalled();
  });
  it("handles an empty scene list", async () => {
    fixture.scenes = [];
    await render();
    await click("Switch scene");
    expect(document.body.textContent).toContain("No scenes in OBS");
  });
  it("reports a failed scene query", async () => {
    fixture.error = true;
    await render();
    await click("Switch scene");
    expect(document.querySelector('[role="alert"]')?.textContent).toBe("Scenes unavailable");
  });
  it("omits actions for unauthorized, disconnected and paused switchers", async () => {
    for (const patch of [
      { can_control: false },
      { obs_connected: false },
      { paused: true },
      { connected: false },
    ]) {
      await render(patch);
      expect(button("Fix feed")).toBeUndefined();
      expect(button("Switch scene")).toBeUndefined();
      expect(document.querySelector("a")?.textContent).toBe("Open Live");
    }
  });
  it("disables controls during reconnection", async () => {
    fixture.connection = "reconnecting";
    await render();
    expect(button("Fix feed")?.disabled).toBe(true);
    expect(button("Switch scene")?.disabled).toBe(true);
    expect(button("Stop stream")?.disabled).toBe(true);
  });
});
