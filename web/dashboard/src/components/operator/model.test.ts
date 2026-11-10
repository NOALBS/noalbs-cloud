import { describe, expect, it } from "vitest";
import type { MyLive, OrganizationLive, SwitcherSummary } from "@/api/types";
import {
  classifyAttention,
  groupItems,
  normalizeMyLive,
  normalizeOrganizationLive,
  normalizeWorkspace,
  type OperatorItem,
} from "./model";

const item = (overrides: Partial<OperatorItem> = {}): OperatorItem => ({
  switcher_id: "switcher",
  switcher_name: "Main",
  workspace_id: "workspace",
  workspace_name: "Studio",
  workspace_slug: "studio",
  connected: true,
  can_control: true,
  obs_connected: true,
  streaming: false,
  recording: false,
  paused: false,
  state: "idle",
  ...overrides,
});
const old = "2026-01-01T10:00:00Z";
const recent = "2026-01-01T11:00:00Z";

describe("operator model", () => {
  it("matches the API's attention rules, including lost mid-stream", () => {
    expect(classifyAttention({ streaming: false, state: "offline" }).attention).toBeNull();
    expect(classifyAttention({ streaming: true, state: "offline", state_since: old })).toEqual({
      attention: "no_feed",
      attention_since: old,
    });
    expect(classifyAttention({ streaming: true, state: "low" }).attention).toBe("low");
    expect(classifyAttention({ streaming: true, state: "live" }).attention).toBeNull();
    expect(
      classifyAttention({
        streaming: false,
        state: "idle",
        obs_lost: { since: recent, stream_started_at: null },
      }).attention,
    ).toBeNull();
    expect(
      classifyAttention({
        streaming: false,
        state: "idle",
        obs_lost: { since: recent, stream_started_at: old },
      }),
    ).toEqual({ attention: "obs_lost", attention_since: recent });
  });

  it("groups and orders longest attention, longest streams, names and most recent offline", () => {
    const groups = groupItems([
      item({ switcher_id: "new-attention", attention: "low", attention_since: recent }),
      item({ switcher_id: "never", connected: false }),
      item({ switcher_id: "new-stream", streaming: true, stream_started_at: recent }),
      item({ switcher_id: "old-attention", attention: "obs_lost", attention_since: old }),
      item({ switcher_id: "old-stream", streaming: true, stream_started_at: old }),
      item({ switcher_id: "offline", connected: false, last_seen_at: recent }),
      item({ switcher_id: "paused", paused: true }),
    ]);
    expect(groups.attention.map((row) => row.switcher_id)).toEqual([
      "old-attention",
      "new-attention",
    ]);
    expect(groups.streaming.map((row) => row.switcher_id)).toEqual(["old-stream", "new-stream"]);
    expect(groups.idle[0]?.switcher_id).toBe("paused");
    expect(groups.offline.map((row) => row.switcher_id)).toEqual(["offline", "never"]);
    expect(groupItems([])).toEqual({ attention: [], streaming: [], idle: [], offline: [] });
  });

  it("searches all names, preserves the legacy OBS filter and keeps offline independent of connected filters", () => {
    const rows = [
      item({ streaming: true }),
      item({ switcher_id: "low", attention: "low", obs_connected: false }),
      item({ switcher_id: "off", connected: false }),
    ];
    expect(groupItems(rows, { show: "attention" }).attention).toHaveLength(1);
    expect(groupItems(rows, { show: "live" }).streaming).toHaveLength(1);
    expect(groupItems(rows, { show: "obs" }).attention).toHaveLength(0);
    expect(groupItems(rows, { show: "live" }).offline).toHaveLength(1);
    expect(groupItems(rows, { q: " STUDIO " }).offline).toHaveLength(1);
    expect(groupItems(rows, { q: "missing" }).offline).toHaveLength(0);
  });

  it("normalizes both live sources with role-based control", () => {
    const base = {
      items: [item({ role: "operator" })],
      offline: [
        {
          switcher_id: "off",
          switcher_name: "Off",
          workspace_id: "workspace",
          workspace_name: "Studio",
          workspace_slug: "studio",
          paused: false,
          role: "operator" as const,
        },
      ],
      counts: {
        live: 1,
        attention: 0,
        connected: 1,
        recording: 0,
        switchers: 2,
        low: 0,
        offline: 0,
        paused: 0,
        obs_connected: 1,
      },
      total: 1,
      per_page: 50,
      page: 1,
      offline_total: 1,
    };
    const mine: MyLive = {
      ...base,
      items: [{ ...item(), role: "operator" }],
      counts: { ...base.counts, workspaces: 1 },
    };
    expect(normalizeMyLive(mine).items[0]?.can_control).toBe(true);
    expect(normalizeMyLive(mine).items[1]?.connected).toBe(false);
    const org: OrganizationLive = { ...base, counts: { ...base.counts, clients: 1 } };
    expect(normalizeOrganizationLive(org, "member").items[0]?.can_control).toBe(true);
    expect(normalizeOrganizationLive(org, null).items[0]?.can_control).toBe(false);
    expect(normalizeOrganizationLive(org, null).counts.clients).toBe(1);
  });

  it("derives workspace attention from status and handles absent status and permissions", () => {
    const summary: SwitcherSummary = {
      id: "one",
      name: "Main",
      paused: false,
      created_at: old,
      stream_server_count: 0,
      revoked_channel_count: 0,
      status: null,
    };
    const data = normalizeWorkspace(
      [summary],
      { id: "workspace", name: "Studio", url_slug: "studio" },
      { role: null, staff: false },
    );
    expect(data.offlineTotal).toBe(1);
    expect(data.items[0]?.can_control).toBe(false);
    expect(data.counts.connected).toBe(0);
    const connected = normalizeWorkspace(
      [
        {
          ...summary,
          status: {
            updated_at: recent,
            auto_switch: true,
            browser_source: true,
            obs_connected: true,
            streaming: true,
            recording: true,
            recording_paused: false,
            state: "offline",
            state_since: old,
            stops_at: recent,
          },
        },
      ],
      { id: "workspace", name: "Studio", url_slug: "studio" },
      { role: "operator", staff: false },
    );
    expect(connected.items[0]?.attention).toBe("no_feed");
    expect(connected.items[0]?.attention_since).toBe(old);
    expect(connected.items[0]?.stops_at).toBe(recent);
    expect(connected.items[0]?.can_control).toBe(true);
    expect(connected.counts).toMatchObject({
      attention: 1,
      connected: 1,
      streaming: 1,
      recording: 1,
    });
    expect(
      normalizeWorkspace([], { id: "w", name: "W", url_slug: "w" }, { role: "owner", staff: false })
        .items,
    ).toEqual([]);
  });
});
