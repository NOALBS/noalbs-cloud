import { t } from "@lingui/core/macro";
import type { ChatBot, ChatPlatform, WorkspaceChatBots } from "@/api/types";

/**
 * Chat grants are browser redirects through the API: it sends the user to
 * Twitch/Kick/YouTube and back to `returnTo` with `?chat_connected=<platform>` or
 * `?chat_error=<code>`.
 */
const withReturn = (path: string, returnTo: string) =>
  `/api${path}?redirect=${encodeURIComponent(returnTo)}`;

/** Lets the bot into a channel (signed in as its owner) and links it to the switcher. */
export const channelConnectUrl = (
  workspaceId: string,
  switcherId: string,
  platform: ChatPlatform,
  returnTo: string,
) =>
  withReturn(
    `/workspaces/${workspaceId}/switchers/${switcherId}/chat/connect/${platform}`,
    returnTo,
  );

/** Connects the workspace's own bot account (signed in as the bot). */
export const workspaceBotConnectUrl = (
  workspaceId: string,
  platform: ChatPlatform,
  returnTo: string,
) => withReturn(`/workspaces/${workspaceId}/chat-bot/connect/${platform}`, returnTo);

/** Connects an organization's bot account, which speaks for all its client workspaces. */
export const organizationBotConnectUrl = (
  organizationId: string,
  platform: ChatPlatform,
  returnTo: string,
) => withReturn(`/organizations/${organizationId}/chat-bot/connect/${platform}`, returnTo);

/** Connects the shared NOALBS bot account (staff). */
export const sharedBotConnectUrl = (platform: ChatPlatform, returnTo: string) =>
  withReturn(`/admin/chat-bots/connect/${platform}`, returnTo);

const grantErrors = (): Record<string, string> => ({
  oauth_denied: t`Connecting was cancelled.`,
  missing_scopes: t`Not every permission was allowed. Try again and allow them all.`,
  plan_limit: t`Your plan doesn’t include your own chat bot.`,
  forbidden: t`You can’t connect that here.`,
  provider_not_configured: t`That platform isn’t set up on this server.`,
  oauth_state: t`That took too long or was started in another tab. Try again.`,
  missing_refresh_token: t`YouTube didn’t allow offline access. Remove NOALBS under your Google account’s third-party access, then connect again.`,
  no_youtube_channel: t`That Google account has no YouTube channel. Pick the account (or brand account) you stream with.`,
});

export function grantErrorMessage(code: string): string {
  return grantErrors()[code] ?? t`Couldn’t connect. Please try again.`;
}

/** Whose bot answers: the workspace's own, its organization's, or the shared NOALBS one. */
export type BotSource = "own" | "organization" | "shared";

/**
 * Who answers in a workspace's channels on `platform`, as the chat service picks it:
 * the workspace's own bot (when allowed), else its organization's, else the shared one.
 */
export function speakingBot(
  bots: WorkspaceChatBots,
  platform: ChatPlatform,
): { bot: ChatBot; source: BotSource } | null {
  const active = (list: ChatBot[]) =>
    list.find((bot) => bot.platform === platform && bot.status === "active");
  const own = bots.custom_allowed ? active(bots.own) : undefined;
  if (own) return { bot: own, source: "own" };
  const organization = active(bots.organization);
  if (organization) return { bot: organization, source: "organization" };
  const shared = active(bots.shared);
  return shared ? { bot: shared, source: "shared" } : null;
}
