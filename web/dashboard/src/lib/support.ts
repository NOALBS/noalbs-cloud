import type { SupportLevel, SupportLogEntry, Workspace } from "@/api/types";

export const SUPPORT_LEVEL_LABELS: Record<SupportLevel, string> = {
  view: "View only",
  control: "View and control",
};

export const SUPPORT_LEVEL_DESCRIPTIONS: Record<SupportLevel, string> = {
  view: "Switchers, their settings, the Live page and history. Nothing can be changed.",
  control: "Also switching scenes, starting and stopping, and changing switcher settings.",
};

/** How long a grant can last, in hours. */
export const SUPPORT_DURATIONS = [
  { hours: 24, label: "24 hours" },
  { hours: 7 * 24, label: "7 days" },
] as const;

const WORKSPACE_PREFIX = "/workspaces/{workspace_id}";
const SWITCHER_PREFIX = "/switchers/{switcher_id}";

/** [method, route after `/workspaces/{workspace_id}/switchers/{switcher_id}`, label]. */
const SWITCHER_LABELS: [string, string, string][] = [
  ["PATCH", "", "Renamed the switcher"],
  ["DELETE", "", "Deleted the switcher"],
  ["GET", "/live", "Watched the Live page"],
  ["GET", "/obs/screenshot", "Watched the OBS preview"],
  ["GET", "/obs/audio/live", "Watched the audio meters"],
  ["GET", "/streams", "Looked at stream history"],
  ["GET", "/streams/{stream_id}", "Looked at a stream"],
  ["GET", "/events", "Looked at the activity"],
  ["GET", "/stream-servers", "Looked at the stream servers"],
  ["POST", "/obs/scene", "Switched scene"],
  ["POST", "/obs/stream", "Started or stopped the stream"],
  ["POST", "/obs/record", "Started or stopped recording"],
  ["POST", "/obs/fix", "Fixed the feed"],
  ["POST", "/obs/source", "Showed or hid a source"],
  ["POST", "/obs/audio/mute", "Muted or unmuted audio"],
  ["PUT", "/obs/audio/volume", "Changed a volume"],
  ["PUT", "/auto-switch", "Turned auto switch on or off"],
  ["PUT", "/triggers", "Changed the switching triggers"],
  ["PUT", "/behaviour", "Changed the switching behaviour"],
  ["PUT", "/scenes", "Changed the scenes"],
  ["PUT", "/obs", "Changed the OBS connection"],
  ["PUT", "/belabox", "Changed the BELABOX Cloud settings"],
  ["PUT", "/moblin", "Changed the Moblin settings"],
  ["POST", "/stream-servers", "Added a stream server"],
  ["PATCH", "/stream-servers/{stream_server_id}", "Changed a stream server"],
  ["DELETE", "/stream-servers/{stream_server_id}", "Removed a stream server"],
  ["PUT", "/stream-servers/{stream_server_id}/overrides", "Changed a stream server’s scenes"],
  ["PUT", "/stream-servers/order", "Reordered the stream servers"],
  ["POST", "/stream-servers/test", "Tested a stream server"],
  ["DELETE", "/streams/{stream_id}", "Deleted a stream from history"],
  // Always refused for support, so only ever attempts.
  ["POST", "/connect-token", "Tried to create a new browser-source link"],
  ["GET", "/moblin/connection", "Tried to see the Moblin password"],
  ["GET", "/streams/{stream_id}/raw", "Tried to see raw stats"],
];

/** [method, route after `/workspaces/{workspace_id}`, label]. */
const WORKSPACE_LABELS: [string, string, string][] = [
  ["GET", "/switchers", "Looked at the switchers"],
  ["POST", "/switchers", "Added a switcher"],
  ["GET", "/members", "Looked at the members"],
  ["GET", "/plan", "Looked at the plan"],
  ["GET", "/support-access", "Looked at support access"],
  ["GET", "/support-access/log", "Looked at this log"],
  ["PUT", "/support-access", "Tried to change support access"],
  ["DELETE", "/support-access", "Tried to change support access"],
  // Support can't do these any more; they're in logs from before.
  ["PATCH", "", "Renamed the workspace"],
  ["DELETE", "", "Deleted the workspace"],
  ["POST", "/transfer", "Transferred ownership"],
  ["GET", "/invites", "Looked at the invites"],
  ["POST", "/invites", "Created an invite"],
  ["DELETE", "/invites/{invite_id}", "Revoked an invite"],
  ["PATCH", "/members/{user_id}", "Changed a member’s role"],
  ["DELETE", "/members/{user_id}", "Removed a member"],
  ["POST", "/billing/checkout", "Started a checkout"],
  ["POST", "/billing/portal", "Opened the billing portal"],
  ["PUT", "/billing/plan", "Changed the plan"],
  ["PUT", "/billing/interval", "Changed the billing interval"],
  ["PUT", "/billing/addons/{addon_code}", "Changed an add-on"],
  ["POST", "/billing/resume", "Resumed the subscription"],
  ["DELETE", "/chat-bot/{platform}", "Removed the workspace’s chat bot"],
];

/** Staff actions from Admin that touched the workspace: [method, route, label]. */
const ADMIN_LABELS: [string, string, string][] = [
  ["PUT", "/admin/workspaces/{workspace_id}/suspension", "Suspended the workspace"],
  ["DELETE", "/admin/workspaces/{workspace_id}/suspension", "Lifted the suspension"],
  ["PUT", "/admin/workspaces/{workspace_id}/plan", "Set the plan"],
  ["POST", "/admin/workspaces/{workspace_id}/break-glass", "Opened emergency access"],
  ["POST", "/admin/switchers/{switcher_id}/reconnect", "Reconnected the browser source"],
  ["DELETE", "/admin/chat-channels/{channel_id}/links", "Unlinked a chat channel"],
];

/** What a support log entry means, in words. */
export function describeSupportAccess(entry: Pick<SupportLogEntry, "method" | "route">): string {
  const admin = ADMIN_LABELS.find(
    ([method, path]) => method === entry.method && path === entry.route,
  );
  if (admin) return admin[2];
  const inWorkspace = entry.route.startsWith(WORKSPACE_PREFIX)
    ? entry.route.slice(WORKSPACE_PREFIX.length)
    : entry.route;
  const onSwitcher =
    inWorkspace === SWITCHER_PREFIX || inWorkspace.startsWith(`${SWITCHER_PREFIX}/`);
  const route = onSwitcher ? inWorkspace.slice(SWITCHER_PREFIX.length) : inWorkspace;
  const labels = onSwitcher ? SWITCHER_LABELS : WORKSPACE_LABELS;
  const known = labels.find(([method, path]) => method === entry.method && path === route);
  if (known) return known[2];

  if (onSwitcher && route.startsWith("/belabox/")) return "Controlled BELABOX";
  if (onSwitcher && route.startsWith("/moblin/")) return "Controlled Moblin";
  if (onSwitcher && route.startsWith("/chat")) {
    return entry.method === "GET" ? "Looked at chat settings" : "Changed chat settings";
  }
  if (entry.method === "GET") return "Looked around";
  return `Changed something (${entry.method} ${inWorkspace || "/"})`;
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
  if (entry.route.startsWith("/admin/")) return { label: "Staff action", tone: "emergency" };
  if (entry.grant_level && entry.break_glass)
    return { label: "Emergency access", tone: "emergency" };
  if (entry.grant_level) return { label: "With permission", tone: "granted" };
  // Before support needed permission: there may have been one that didn't cover it.
  if (entry.grant_in_force) return { label: "Beyond your permission", tone: "none" };
  return { label: "Without permission", tone: "none" };
}
