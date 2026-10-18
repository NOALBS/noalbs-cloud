import type { AdminAuditEntry, AdminStatus, ConnectedSwitcher } from "@/api/types";
import { describeSupportAccess } from "./support";

/** "9.50" or "9,50" → 950 cents; `null` for anything that isn't a positive amount. */
export function toCents(input: string): number | null {
  const normalized = input.trim().replace(",", ".");
  if (!/^\d+(\.\d{1,2})?$/.test(normalized)) return null;
  const cents = Math.round(Number(normalized) * 100);
  return cents > 0 ? cents : null;
}

/** 950 → "9.50", 900 → "9" (for editing a price). */
export function fromCents(cents: number): string {
  return cents % 100 === 0 ? String(cents / 100) : (cents / 100).toFixed(2);
}

export function pageCount(total: number, perPage: number): number {
  return Math.max(1, Math.ceil(total / Math.max(perPage, 1)));
}

/** Lowercase letters, digits and single dashes, 2–30 long (mirrors the API). */
export function validCode(code: string): boolean {
  return /^[a-z0-9]+(-[a-z0-9]+)*$/.test(code) && code.length >= 2 && code.length <= 30;
}

/** A plan or add-on name as a code: "Backup Ingest" → "backup-ingest". */
export function codeFromName(name: string): string {
  return name
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 30)
    .replace(/-+$/, "");
}

/** Something on the status page that needs a look, most serious first. */
export interface StatusProblem {
  severity: "error" | "warning";
  text: string;
}

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** What on Admin → Status needs attention; empty when all is well. */
export function statusProblems(status: AdminStatus): StatusProblem[] {
  const problems: StatusProblem[] = [];
  const error = (text: string) => problems.push({ severity: "error", text });
  const warning = (text: string) => problems.push({ severity: "warning", text });
  const chat = status.chat;

  if (status.switchers_error) error(`Couldn’t read switcher statuses: ${status.switchers_error}`);
  const chatNeeded =
    chat.active_channels > 0 && (chat.twitch != null || chat.kick != null || chat.youtube != null);
  if (chat.instances_error) {
    (chatNeeded ? error : warning)(chat.instances_error);
  } else if (chatNeeded && chat.instances.length === 0) {
    error("No chat service is running: chat commands aren’t being answered.");
  }

  const twitch = chat.twitch;
  if (twitch) {
    if (twitch.error) error(`Twitch: ${twitch.error}`);
    const down = twitch.shards.filter((shard) => shard.status !== "enabled");
    if (down.length > 0) {
      error(
        `${plural(down.length, "Twitch shard")} of ${twitch.shards.length} not enabled (${[
          ...new Set(down.map((shard) => shard.status)),
        ].join(", ")})`,
      );
    }
    const unheld = twitch.shards.filter((shard) => !shard.instance);
    if (unheld.length > 0 && chat.instances.length > 0) {
      warning(`${plural(unheld.length, "Twitch shard")} not held by any chat instance`);
    }
    if (twitch.max_total_cost > 0 && twitch.total_cost >= twitch.max_total_cost * 0.8) {
      warning(
        `EventSub cost at ${Math.round((twitch.total_cost / twitch.max_total_cost) * 100)}% of Twitch’s limit`,
      );
    }
    const onConduit = twitch.conduit_subscriptions_by_type.reduce((sum, c) => sum + c.count, 0);
    if (!twitch.error && twitch.conduit_id && onConduit !== twitch.recorded) {
      warning(
        `Twitch has ${plural(onConduit, "subscription")} on our conduit, the chat service recorded ${twitch.recorded} (the next reconcile fixes this)`,
      );
    }
  }

  const kick = chat.kick;
  if (kick) {
    if (kick.error) error(`Kick: ${kick.error}`);
    if (kick.subscriptions != null && kick.subscriptions !== kick.recorded) {
      warning(
        `Kick has ${plural(kick.subscriptions, "subscription")}, the chat service recorded ${kick.recorded} (the next reconcile fixes this)`,
      );
    }
  }

  const youtube = chat.youtube;
  if (youtube) {
    const now = Date.parse(status.generated_at);
    const watcher = youtube.watcher;
    if (youtube.error) {
      warning(`YouTube: ${youtube.error}`);
    } else if (
      chat.instances.length > 0 &&
      (!watcher || now - Date.parse(watcher.updated_at) > 30_000)
    ) {
      warning(
        "No chat instance is following YouTube chats (is YouTube configured on the chat service?)",
      );
    }
    const last = youtube.last_error;
    const age = last ? now - Date.parse(last.at) : Number.POSITIVE_INFINITY;
    if (last?.quota && age < 24 * 3_600_000) {
      error(`YouTube’s daily quota ran out (${last.message}); it resets at midnight Pacific time`);
    } else if (last && age < 3_600_000) {
      warning(`YouTube: ${last.message}`);
    }
  }

  if (chat.revoked.length > 0) {
    const them = chat.revoked.length === 1 ? "it needs" : "they need";
    warning(`${plural(chat.revoked.length, "chat grant")} revoked: ${them} connecting again`);
  }
  return problems.sort((a, b) => (a.severity === b.severity ? 0 : a.severity === "error" ? -1 : 1));
}

/** Which connected switchers Admin → Status lists. */
export type ConnectedFilter = "all" | "live" | "obs" | "no-obs";

export const CONNECTED_FILTERS: { value: ConnectedFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "live", label: "Live" },
  { value: "obs", label: "OBS connected" },
  { value: "no-obs", label: "No OBS" },
];

export function matchesConnected(switcher: ConnectedSwitcher, filter: ConnectedFilter): boolean {
  switch (filter) {
    case "all":
      return true;
    case "live":
      return switcher.streaming;
    case "obs":
      return switcher.obs_connected;
    case "no-obs":
      return !switcher.obs_connected;
  }
}

const ADMIN_ACTIONS: Record<string, string> = {
  suspend_workspace: "Suspended",
  lift_suspension: "Lifted the suspension of",
  set_plan: "Set the plan of",
  break_glass: "Opened emergency access to",
  reconnect_switcher: "Reconnected a switcher in",
  unlink_chat_channel: "Unlinked a chat channel from",
  sign_out_everywhere: "Signed out everywhere:",
  disable_user: "Disabled",
  enable_user: "Enabled",
  set_staff: "Changed staff access of",
  create_notice: "Posted a notice",
  update_notice: "Changed a notice",
  end_notice: "Ended a notice",
  delete_notice: "Deleted a notice",
  create_organization: "Created",
  update_organization: "Changed the limits of",
  delete_organization: "Deleted",
  suspend_organization: "Suspended",
  lift_organization_suspension: "Lifted the suspension of",
  invite_to_organization: "Invited someone to the team of",
};

/** What an audit entry says staff did, in words. */
export function describeAudit(
  entry: Pick<
    AdminAuditEntry,
    "kind" | "action" | "method" | "workspace_name" | "target_user_name" | "target_user_id"
  >,
): string {
  if (entry.kind === "support") {
    const what = describeSupportAccess({ method: entry.method ?? "GET", route: entry.action });
    return entry.workspace_name ? `${what} in ${entry.workspace_name}` : what;
  }
  const verb = ADMIN_ACTIONS[entry.action] ?? entry.action;
  const target =
    entry.workspace_name ??
    entry.target_user_name ??
    (entry.target_user_id ? "a deleted user" : "");
  return target ? `${verb} ${target}` : verb;
}
