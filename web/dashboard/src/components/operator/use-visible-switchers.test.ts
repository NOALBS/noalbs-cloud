import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import { api } from "@/api/client";
import type { MyLive, SwitcherSummary, Workspace } from "@/api/types";
import {
  fetchVisibleSwitchers,
  mergeVisibleSwitchers,
  useVisibleSwitchers,
} from "./use-visible-switchers";

vi.mock("@/api/client", () => ({ api: { GET: vi.fn() }, unwrap: async (value: unknown) => value }));
const workspace = { id: "w", name: "Studio", url_slug: "studio" } as Workspace;
const actor = { role: null, organization: "member", staff: false } as const;
function Probe() {
  useVisibleSwitchers({ workspace, actor });
  useVisibleSwitchers({ workspace, actor });
  return null;
}
const summary = (id: string, status: SwitcherSummary["status"] = null): SwitcherSummary => ({
  id,
  name: id,
  paused: false,
  created_at: "2026-01-01T00:00:00Z",
  last_seen_at: null,
  status,
  server: null,
  stream_server_count: 0,
  revoked_channel_count: 0,
});
const status = {
  updated_at: "2026-01-01T00:00:00Z",
  auto_switch: true,
  browser_source: true,
  obs_connected: true,
  streaming: true,
  recording: false,
  recording_paused: false,
  state: "offline" as const,
  state_since: "2026-01-01T00:00:00Z",
};
const personal: MyLive = {
  counts: {
    live: 0,
    attention: 0,
    connected: 0,
    obs_connected: 0,
    recording: 0,
    workspaces: 1,
    switchers: 1,
    low: 0,
    offline: 0,
    paused: 0,
  },
  items: [],
  offline: [
    {
      switcher_id: "same",
      switcher_name: "Old name",
      workspace_id: "w",
      workspace_name: "Studio",
      workspace_slug: "studio",
      role: "owner",
      paused: false,
      last_seen_at: null,
    },
  ],
  offline_total: 1,
  total: 0,
  page: 1,
  per_page: 50,
};
describe("visible switcher sources", () => {
  it("deduplicates by id, prefers current summaries and includes non-member workspaces", () => {
    const result = mergeVisibleSwitchers(personal, {
      workspace,
      actor,
      switchers: [summary("same", status), summary("org-only", { ...status, state: "live" })],
    });
    expect(result.items.map((item) => item.switcher_id)).toEqual(["same", "org-only"]);
    expect(result.items[0]?.switcher_name).toBe("same");
    expect(result.items[0]?.can_control).toBe(true);
    expect(result.attention).toBe(1);
    expect(result.total).toBe(2);
    expect(result.offline).toBe(0);
  });
  it("handles empty/error fallbacks, single switchers and capped offline totals", () => {
    expect(mergeVisibleSwitchers(undefined, { workspace, actor }).total).toBe(0);
    expect(
      mergeVisibleSwitchers(undefined, { workspace, actor, switchers: [summary("only")] }),
    ).toMatchObject({ total: 1, offline: 1, attention: 0 });
    expect(
      mergeVisibleSwitchers({ ...personal, offline_total: 120 }, { workspace, actor }),
    ).toMatchObject({ total: 120, offline: 120 });
    const extra = [summary("recovered", status)];
    expect(
      mergeVisibleSwitchers(
        { ...personal, offline_total: 120 },
        { workspace: { ...workspace, role: "owner" }, actor, switchers: extra },
      ),
    ).toMatchObject({ total: 120, offline: 119 });
    expect(
      mergeVisibleSwitchers(
        { ...personal, offline_total: 120 },
        { workspace, actor, switchers: extra },
      ),
    ).toMatchObject({ total: 121, offline: 120 });
  });
  it("reads all connected pages without duplicating the offline roster", async () => {
    const row: MyLive["items"][number] = {
      switcher_id: "first",
      switcher_name: "Main",
      workspace_id: "w",
      workspace_name: "Studio",
      workspace_slug: "studio",
      role: "owner",
      obs_connected: true,
      streaming: true,
      recording: false,
      paused: false,
      state: "live",
    };
    vi.mocked(api.GET).mockReset();
    vi.mocked(api.GET)
      .mockResolvedValueOnce({ ...personal, total: 2, per_page: 1, items: [row] } as never)
      .mockResolvedValueOnce({ ...personal, items: [{ ...row, switcher_id: "second" }] } as never);
    const controller = new AbortController();
    const result = await fetchVisibleSwitchers(controller.signal);
    expect(api.GET).toHaveBeenCalledTimes(2);
    expect(api.GET).toHaveBeenLastCalledWith("/me/live", {
      params: { query: { page: 2 } },
      signal: controller.signal,
    });
    expect(result.offline).toHaveLength(1);
    expect(result.items.map((item) => item.switcher_id)).toEqual(["first", "second"]);
  });
  it("propagates failed page requests so the hook can fall back to current summaries", async () => {
    vi.mocked(api.GET).mockReset();
    vi.mocked(api.GET).mockRejectedValueOnce(new Error("Unavailable"));
    await expect(fetchVisibleSwitchers(new AbortController().signal)).rejects.toThrow(
      "Unavailable",
    );
  });
  it("shares one query between observers, polls every ten seconds and pauses while hidden", async () => {
    vi.useFakeTimers();
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    let hidden = false;
    const visibility = vi.spyOn(document, "hidden", "get").mockImplementation(() => hidden);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    vi.mocked(api.GET).mockReset();
    vi.mocked(api.GET).mockResolvedValue(personal as never);
    try {
      await act(async () =>
        root.render(createElement(QueryClientProvider, { client }, createElement(Probe))),
      );
      await act(async () => vi.advanceTimersByTimeAsync(1));
      expect(api.GET).toHaveBeenCalledTimes(1);
      await act(async () => vi.advanceTimersByTimeAsync(10000));
      expect(api.GET).toHaveBeenCalledTimes(2);
      await act(async () => {
        hidden = true;
        document.dispatchEvent(new Event("visibilitychange"));
      });
      await act(async () => vi.advanceTimersByTimeAsync(20000));
      expect(api.GET).toHaveBeenCalledTimes(2);
      await act(async () => {
        hidden = false;
        document.dispatchEvent(new Event("visibilitychange"));
      });
      await act(async () => vi.advanceTimersByTimeAsync(1));
      expect(api.GET).toHaveBeenCalledTimes(3);
    } finally {
      await act(async () => root.unmount());
      client.clear();
      host.remove();
      visibility.mockRestore();
      vi.useRealTimers();
      vi.unstubAllGlobals();
    }
  });
});
