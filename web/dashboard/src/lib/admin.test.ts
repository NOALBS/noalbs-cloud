import { describe, expect, it } from "vitest";
import type { AdminStatus, ConnectedSwitcher } from "@/api/types";
import {
  type ConnectedFilter,
  codeFromName,
  describeAudit,
  fromCents,
  matchesConnected,
  pageCount,
  statusProblems,
  toCents,
  validCode,
} from "./admin";

describe("prices in the admin forms", () => {
  it("reads amounts as cents", () => {
    expect(toCents("9")).toBe(900);
    expect(toCents("9.5")).toBe(950);
    expect(toCents("9,50")).toBe(950);
    expect(toCents(" 12.99 ")).toBe(1299);
  });

  it("refuses anything else", () => {
    expect(toCents("0")).toBeNull();
    expect(toCents("-5")).toBeNull();
    expect(toCents("9.999")).toBeNull();
    expect(toCents("abc")).toBeNull();
    expect(toCents("")).toBeNull();
  });

  it("shows cents for editing", () => {
    expect(fromCents(900)).toBe("9");
    expect(fromCents(950)).toBe("9.50");
  });
});

describe("codes", () => {
  it("are derived from names and checked like the API", () => {
    expect(codeFromName("Backup Ingest")).toBe("backup-ingest");
    expect(codeFromName("  Pro+ ")).toBe("pro");
    expect(validCode("backup-ingest")).toBe(true);
    expect(validCode("Pro")).toBe(false);
    expect(validCode("a")).toBe(false);
    expect(validCode("a--b")).toBe(false);
  });
});

describe("pageCount", () => {
  it("has at least one page", () => {
    expect(pageCount(0, 25)).toBe(1);
    expect(pageCount(26, 25)).toBe(2);
  });
});

describe("statusProblems", () => {
  const healthy = (): AdminStatus => ({
    generated_at: "2026-09-25T10:00:00Z",
    switchers: {
      total: 3,
      connected: 2,
      obs_connected: 2,
      streaming: 1,
      recording: 0,
      paused: 0,
      low: 0,
      offline: 0,
    },
    connected: [],
    switchers_error: null,
    chat: {
      environment: "env1",
      instances: [{ id: "chat-a", last_seen_seconds: 4, shards: [0, 1] }],
      instances_error: null,
      active_channels: 2,
      links: 2,
      twitch: {
        conduit_id: "c1",
        shards: [
          { id: "0", status: "enabled", instance: "chat-a" },
          { id: "1", status: "enabled", instance: "chat-a" },
        ],
        subscriptions_by_status: [{ name: "enabled", count: 3 }],
        conduit_subscriptions_by_type: [
          { name: "channel.chat.message", count: 2 },
          { name: "user.authorization.revoke", count: 1 },
        ],
        total_cost: 0,
        max_total_cost: 10000,
        recorded: 3,
        error: null,
      },
      kick: { subscriptions: 1, recorded: 1, error: null },
      youtube: {
        watcher: {
          instance_id: "chat-a",
          updated_at: "2026-09-25T09:59:55Z",
          streaming: 1,
          chats_open: 1,
        },
        last_error: null,
        error: null,
      },
      revoked: [],
    },
  });

  it("is empty when all is well", () => {
    expect(statusProblems(healthy())).toEqual([]);
  });

  it("puts a missing chat service and broken shards first", () => {
    const status = healthy();
    status.chat.instances = [];
    status.chat.revoked = [
      {
        kind: "channel",
        platform: "twitch",
        login: "kate",
        workspace_id: null,
        workspace_name: null,
        switchers: 1,
        revoked_at: "2026-09-25T09:00:00Z",
      },
    ];
    if (status.chat.twitch) {
      status.chat.twitch.shards[1] = { id: "1", status: "websocket_disconnected", instance: null };
      status.chat.twitch.total_cost = 9000;
    }
    const problems = statusProblems(status);
    expect(problems.map((problem) => problem.severity)).toEqual([
      "error",
      "error",
      "warning",
      "warning",
    ]);
    expect(problems.map((problem) => problem.text)).toEqual([
      "No chat service is running: chat commands aren’t being answered.",
      "1 Twitch shard of 2 not enabled (websocket_disconnected)",
      "EventSub cost at 90% of Twitch’s limit",
      "1 chat grant revoked: it needs connecting again",
    ]);
  });

  it("notices the chat service's records drifting from the platforms", () => {
    const status = healthy();
    if (status.chat.kick) status.chat.kick.subscriptions = 0;
    expect(statusProblems(status)).toEqual([
      {
        severity: "warning",
        text: "Kick has 0 subscriptions, the chat service recorded 1 (the next reconcile fixes this)",
      },
    ]);
  });

  it("doesn't need a chat service when nothing uses chat", () => {
    const status = healthy();
    status.chat.instances = [];
    status.chat.active_channels = 0;
    expect(statusProblems(status)).toEqual([]);
  });

  it("says when YouTube's quota ran out or nobody follows its chats", () => {
    const status = healthy();
    if (!status.chat.youtube) throw new Error("no YouTube");
    status.chat.youtube.last_error = {
      at: "2026-09-25T08:00:00Z",
      quota: true,
      message: "quotaExceeded",
    };
    status.chat.youtube.watcher = null;
    expect(statusProblems(status)).toEqual([
      {
        severity: "error",
        text: "YouTube’s daily quota ran out (quotaExceeded); it resets at midnight Pacific time",
      },
      {
        severity: "warning",
        text: "No chat instance is following YouTube chats (is YouTube configured on the chat service?)",
      },
    ]);
  });
});

describe("matchesConnected", () => {
  const switcher = (patch: Partial<ConnectedSwitcher>): ConnectedSwitcher => ({
    switcher_id: "s",
    switcher_name: "Main",
    workspace_id: "w",
    workspace_name: "Kate",
    workspace_slug: "kate",
    owner_username: "kate",
    obs_connected: false,
    streaming: false,
    recording: false,
    paused: false,
    state: "idle",
    stream_started_at: null,
    scene: null,
    ...patch,
  });

  it("splits by live and OBS", () => {
    const live = switcher({ obs_connected: true, streaming: true, state: "live" });
    const obs = switcher({ obs_connected: true });
    const bare = switcher({});
    const pick = (filter: ConnectedFilter) =>
      [live, obs, bare].filter((s) => matchesConnected(s, filter));
    expect(pick("all")).toEqual([live, obs, bare]);
    expect(pick("live")).toEqual([live]);
    expect(pick("obs")).toEqual([live, obs]);
    expect(pick("no-obs")).toEqual([bare]);
  });
});

describe("describeAudit", () => {
  const entry = {
    kind: "admin",
    action: "suspend_workspace",
    method: null,
    workspace_name: "Kate",
    target_user_name: null,
    target_user_id: null,
  };

  it("names admin actions with what they were about", () => {
    expect(describeAudit(entry)).toBe("Suspended Kate");
    expect(
      describeAudit({
        ...entry,
        action: "disable_user",
        workspace_name: null,
        target_user_name: "spammer",
        target_user_id: "u",
      }),
    ).toBe("Disabled spammer");
    expect(
      describeAudit({ ...entry, action: "enable_user", workspace_name: null, target_user_id: "u" }),
    ).toBe("Enabled a deleted user");
  });

  it("describes support work with the workspace", () => {
    expect(
      describeAudit({
        ...entry,
        kind: "support",
        method: "POST",
        action: "/workspaces/{workspace_id}/switchers/{switcher_id}/obs/scene",
      }),
    ).toBe("Switched scene in Kate");
  });
});
