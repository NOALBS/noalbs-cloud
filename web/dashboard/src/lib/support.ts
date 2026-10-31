import { t } from "@lingui/core/macro";
import type { SupportLevel, SupportLogEntry, Workspace } from "@/api/types";

export const supportLevelLabels = (): Record<SupportLevel, string> => ({
  view: t`View only`,
  control: t`View and control`,
});

export const supportLevelDescriptions = (): Record<SupportLevel, string> => ({
  view: t`Switchers, their settings, the Live page and history. Nothing can be changed.`,
  control: t`Also switching scenes, starting and stopping, and changing switcher settings.`,
});

/** How long a grant can last, in hours. */
export const supportDurations = () =>
  [
    { hours: 24, label: t`24 hours` },
    { hours: 7 * 24, label: t`7 days` },
  ] as const;

const WORKSPACE_PREFIX = "/workspaces/{workspace_id}";
const SWITCHER_PREFIX = "/switchers/{switcher_id}";

/** [method, route after `/workspaces/{workspace_id}/switchers/{switcher_id}`, label]. */
const switcherLabels = (): [string, string, string][] => [
  ["PATCH", "", t`Renamed the switcher`],
  ["DELETE", "", t`Deleted the switcher`],
  ["GET", "/live", t`Watched the Live page`],
  ["GET", "/obs/screenshot", t`Watched the OBS preview`],
  ["GET", "/obs/screenshot/watchers", t`Looked at who watches the OBS preview`],
  ["GET", "/obs/audio/live", t`Watched the audio meters`],
  ["GET", "/streams", t`Looked at stream history`],
  ["GET", "/streams/{stream_id}", t`Looked at a stream`],
  ["GET", "/events", t`Looked at the activity`],
  ["GET", "/stream-servers", t`Looked at the stream servers`],
  ["POST", "/obs/scene", t`Switched scene`],
  ["POST", "/obs/stream", t`Started or stopped the stream`],
  ["POST", "/obs/record", t`Started or stopped recording`],
  ["POST", "/obs/fix", t`Fixed the feed`],
  ["POST", "/obs/source", t`Showed or hid a source`],
  ["POST", "/obs/audio/mute", t`Muted or unmuted audio`],
  ["PUT", "/obs/audio/volume", t`Changed a volume`],
  ["PUT", "/auto-switch", t`Turned auto switch on or off`],
  ["PUT", "/triggers", t`Changed the switching triggers`],
  ["PUT", "/behaviour", t`Changed the switching behaviour`],
  ["PUT", "/scenes", t`Changed the scenes`],
  ["PUT", "/obs", t`Changed the OBS connection`],
  ["PUT", "/belabox", t`Changed the BELABOX Cloud settings`],
  ["PUT", "/moblin", t`Changed the Moblin settings`],
  ["POST", "/stream-servers", t`Added a stream server`],
  ["PATCH", "/stream-servers/{stream_server_id}", t`Changed a stream server`],
  ["DELETE", "/stream-servers/{stream_server_id}", t`Removed a stream server`],
  ["PUT", "/stream-servers/{stream_server_id}/overrides", t`Changed a stream server’s scenes`],
  ["PUT", "/stream-servers/order", t`Reordered the stream servers`],
  ["POST", "/stream-servers/test", t`Tested a stream server`],
  ["DELETE", "/streams/{stream_id}", t`Deleted a stream from history`],
  // Always refused for support, so only ever attempts.
  ["POST", "/connect-token", t`Tried to create a new browser-source link`],
  ["GET", "/moblin/connection", t`Tried to see the Moblin password`],
  ["GET", "/streams/{stream_id}/raw", t`Tried to see raw stats`],
  ["PUT", "/preview", t`Tried to change who sees the OBS preview`],
];

/** [method, route after `/workspaces/{workspace_id}`, label]. */
const workspaceLabels = (): [string, string, string][] => [
  ["GET", "/switchers", t`Looked at the switchers`],
  ["POST", "/switchers", t`Added a switcher`],
  ["GET", "/members", t`Looked at the members`],
  ["GET", "/plan", t`Looked at the plan`],
  ["GET", "/support-access", t`Looked at support access`],
  ["GET", "/support-access/log", t`Looked at this log`],
  ["PUT", "/support-access", t`Tried to change support access`],
  ["DELETE", "/support-access", t`Tried to change support access`],
  // Support can't do these any more; they're in logs from before.
  ["PATCH", "", t`Renamed the workspace`],
  ["DELETE", "", t`Deleted the workspace`],
  ["POST", "/transfer", t`Transferred ownership`],
  ["GET", "/invites", t`Looked at the invites`],
  ["POST", "/invites", t`Created an invite`],
  ["DELETE", "/invites/{invite_id}", t`Revoked an invite`],
  ["PATCH", "/members/{user_id}", t`Changed a member’s role`],
  ["DELETE", "/members/{user_id}", t`Removed a member`],
  ["POST", "/billing/checkout", t`Started a checkout`],
  ["POST", "/billing/portal", t`Opened the billing portal`],
  ["PUT", "/billing/plan", t`Changed the plan`],
  ["PUT", "/billing/interval", t`Changed the billing interval`],
  ["PUT", "/billing/addons/{addon_code}", t`Changed an add-on`],
  ["POST", "/billing/resume", t`Resumed the subscription`],
  ["DELETE", "/chat-bot/{platform}", t`Removed the workspace’s chat bot`],
];

/** Staff actions from Admin that touched the workspace: [method, route, label]. */
const adminLabels = (): [string, string, string][] => [
  ["PUT", "/admin/workspaces/{workspace_id}/suspension", t`Suspended the workspace`],
  ["DELETE", "/admin/workspaces/{workspace_id}/suspension", t`Lifted the suspension`],
  ["PUT", "/admin/workspaces/{workspace_id}/plan", t`Set the plan`],
  ["POST", "/admin/workspaces/{workspace_id}/break-glass", t`Opened emergency access`],
  ["POST", "/admin/switchers/{switcher_id}/reconnect", t`Reconnected the browser source`],
  ["DELETE", "/admin/chat-channels/{channel_id}/links", t`Unlinked a chat channel`],
];

/** What a support log entry means, in words. */
export function describeSupportAccess(entry: Pick<SupportLogEntry, "method" | "route">): string {
  const admin = adminLabels().find(
    ([method, path]) => method === entry.method && path === entry.route,
  );
  if (admin) return admin[2];
  const inWorkspace = entry.route.startsWith(WORKSPACE_PREFIX)
    ? entry.route.slice(WORKSPACE_PREFIX.length)
    : entry.route;
  const onSwitcher =
    inWorkspace === SWITCHER_PREFIX || inWorkspace.startsWith(`${SWITCHER_PREFIX}/`);
  const route = onSwitcher ? inWorkspace.slice(SWITCHER_PREFIX.length) : inWorkspace;
  const labels = onSwitcher ? switcherLabels() : workspaceLabels();
  const known = labels.find(([method, path]) => method === entry.method && path === route);
  if (known) return known[2];

  if (onSwitcher && route.startsWith("/belabox/")) return t`Controlled BELABOX`;
  if (onSwitcher && route.startsWith("/moblin/")) return t`Controlled Moblin`;
  if (onSwitcher && route.startsWith("/chat")) {
    return entry.method === "GET" ? t`Looked at chat settings` : t`Changed chat settings`;
  }
  if (entry.method === "GET") return t`Looked around`;
  return t`Changed something (${entry.method} ${inWorkspace || "/"})`;
}

/** Whether the entry changed something rather than only looked. */
export function isSupportChange(entry: Pick<SupportLogEntry, "method">): boolean {
  return entry.method !== "GET";
}

/**
 * Staff outside the workspace without support access: the layout shows the
 * gate, so loaders shouldn't fetch what the API would refuse.
 */
export function supportGated(
  workspace: Pick<Workspace, "role" | "support_access"> & {
    organization_role?: Workspace["organization_role"];
  },
): boolean {
  // The organization's team runs its client workspaces without any grant.
  return workspace.role == null && !workspace.organization_role && !workspace.support_access;
}

/** Under what a log entry was done, for its badge. */
export function supportAccessBasis(
  entry: Pick<SupportLogEntry, "grant_level" | "break_glass" | "grant_in_force" | "route">,
): { label: string; tone: "granted" | "emergency" | "none" } {
  if (entry.route.startsWith("/admin/")) return { label: t`Staff action`, tone: "emergency" };
  if (entry.grant_level && entry.break_glass)
    return { label: t`Emergency access`, tone: "emergency" };
  if (entry.grant_level) return { label: t`With permission`, tone: "granted" };
  // Before support needed permission: there may have been one that didn't cover it.
  if (entry.grant_in_force) return { label: t`Beyond your permission`, tone: "none" };
  return { label: t`Without permission`, tone: "none" };
}
