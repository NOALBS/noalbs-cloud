import { describe, expect, it } from "vitest";
import type { ChatBot, WorkspaceChatBots } from "@/api/types";
import { channelConnectUrl, grantErrorMessage, speakingBot } from "./chat";

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
