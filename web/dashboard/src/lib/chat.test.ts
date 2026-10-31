import { describe, expect, it } from "vitest";
import type { ChatBot, SwitcherSummary, WorkspaceChatBots } from "@/api/types";
import {
  channelConnectUrl,
  grantErrorMessage,
  revokedOwnBots,
  speakingBot,
  switchersToReconnect,
} from "./chat";

const bot = (login: string, patch: Partial<ChatBot> = {}): ChatBot => ({
  platform: "twitch",
  login,
  display_name: login,
  status: "active",
  connected_at: "2026-09-01T00:00:00Z",
  ...patch,
});

describe("speakingBot", () => {
  const bots: WorkspaceChatBots = {
    custom_allowed: true,
    own: [bot("katebot")],
    organization: [],
    shared: [bot("noalbsbot"), bot("noalbs", { platform: "kick" })],
  };

  it("prefers the workspace's own bot when the plan allows it", () => {
    expect(speakingBot(bots, "twitch")).toEqual({ bot: bots.own[0], source: "own" });
    expect(speakingBot({ ...bots, custom_allowed: false }, "twitch")?.bot.login).toBe("noalbsbot");
  });

  it("falls back to the shared bot when the own one is revoked or missing", () => {
    const revoked = { ...bots, own: [bot("katebot", { status: "revoked" })] };
    expect(speakingBot(revoked, "twitch")?.bot.login).toBe("noalbsbot");
    expect(speakingBot(bots, "kick")).toEqual({ bot: bots.shared[1], source: "shared" });
  });

  it("is null without any bot", () => {
    expect(
      speakingBot({ custom_allowed: false, own: [], organization: [], shared: [] }, "kick"),
    ).toBeNull();
  });

  it("puts the organization's bot between the own bot and the shared one", () => {
    const managed: WorkspaceChatBots = { ...bots, organization: [bot("nomadbot")] };
    expect(speakingBot(managed, "twitch")).toEqual({ bot: managed.own[0], source: "own" });
    const notAllowed = { ...managed, custom_allowed: false };
    expect(speakingBot(notAllowed, "twitch")).toEqual({
      bot: managed.organization[0],
      source: "organization",
    });
    const revoked: WorkspaceChatBots = {
      ...notAllowed,
      organization: [bot("nomadbot", { status: "revoked" })],
    };
    expect(speakingBot(revoked, "twitch")?.source).toBe("shared");
  });
});

describe("revokedOwnBots", () => {
  const bots: WorkspaceChatBots = {
    custom_allowed: true,
    own: [bot("katebot", { status: "revoked" }), bot("katekick", { platform: "kick" })],
    organization: [bot("nomadbot", { status: "revoked" })],
    shared: [bot("noalbsbot", { status: "revoked" })],
  };

  it("lists only the workspace's own refused bots", () => {
    expect(revokedOwnBots(bots)).toEqual([bots.own[0]]);
  });

  it("leaves them out on a plan without own bots, where they don't speak anyway", () => {
    expect(revokedOwnBots({ ...bots, custom_allowed: false })).toEqual([]);
  });
});

describe("switchersToReconnect", () => {
  const switcher = (id: string, revoked: number) =>
    ({ id, revoked_channel_count: revoked }) as SwitcherSummary;
  const switchers = [switcher("a", 0), switcher("b", 1), switcher("c", 2)];

  it("finds the switchers with a refused channel", () => {
    expect(switchersToReconnect(switchers, undefined).map(({ id }) => id)).toEqual(["b", "c"]);
  });

  it("skips the one whose Chat page is open", () => {
    expect(switchersToReconnect(switchers, "b").map(({ id }) => id)).toEqual(["c"]);
  });
});

describe("chat grants", () => {
  it("go through the API with the page to come back to", () => {
    expect(channelConnectUrl("w1", "s1", "kick", "/kate/s/s1/chat")).toBe(
      "/api/workspaces/w1/switchers/s1/chat/connect/kick?redirect=%2Fkate%2Fs%2Fs1%2Fchat",
    );
  });

  it("explain what went wrong", () => {
    expect(grantErrorMessage("missing_scopes")).toMatch(/allow them all/);
    expect(grantErrorMessage("something_new")).toBe("Couldn’t connect. Please try again.");
  });
});
