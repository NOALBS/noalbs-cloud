import { plural, t } from "@lingui/core/macro";
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

/** What on Admin → Status needs attention; empty when all is well. */
export function statusProblems(status: AdminStatus): StatusProblem[] {
  const problems: StatusProblem[] = [];
  const error = (text: string) => problems.push({ severity: "error", text });
  const warning = (text: string) => problems.push({ severity: "warning", text });
  const chat = status.chat;

  if (status.switchers_error) error(t`Couldn’t read switcher statuses: ${status.switchers_error}`);
  const chatNeeded =
    chat.active_channels > 0 && (chat.twitch != null || chat.kick != null || chat.youtube != null);
  if (chat.instances_error) {
    (chatNeeded ? error : warning)(chat.instances_error);
  } else if (chatNeeded && chat.instances.length === 0) {
    error(t`No chat service is running: chat commands aren’t being answered.`);
  }

  const twitch = chat.twitch;
  if (twitch) {
    if (twitch.error) error(`Twitch: ${twitch.error}`);
    const down = twitch.shards.filter((shard) => shard.status !== "enabled");
    if (down.length > 0) {
      error(
        t`${plural(down.length, { one: "# Twitch shard", other: "# Twitch shards" })} of ${twitch.shards.length} not enabled (${[
          ...new Set(down.map((shard) => shard.status)),
        ].join(", ")})`,
      );
    }
    const unheld = twitch.shards.filter((shard) => !shard.instance);
    if (unheld.length > 0 && chat.instances.length > 0) {
      warning(
        plural(unheld.length, {
          one: "# Twitch shard not held by any chat instance",
          other: "# Twitch shards not held by any chat instance",
        }),
      );
    }
    if (twitch.max_total_cost > 0 && twitch.total_cost >= twitch.max_total_cost * 0.8) {
      warning(
        t`EventSub cost at ${Math.round((twitch.total_cost / twitch.max_total_cost) * 100)}% of Twitch’s limit`,
      );
    }
    const onConduit = twitch.conduit_subscriptions_by_type.reduce((sum, c) => sum + c.count, 0);
    if (!twitch.error && twitch.conduit_id && onConduit !== twitch.recorded) {
      warning(
        t`Twitch has ${plural(onConduit, { one: "# subscription", other: "# subscriptions" })} on our conduit, the chat service recorded ${twitch.recorded} (the next reconcile fixes this)`,
      );
    }
  }

  const kick = chat.kick;
  if (kick) {
    if (kick.error) error(`Kick: ${kick.error}`);
    if (kick.subscriptions != null && kick.subscriptions !== kick.recorded) {
      warning(
        t`Kick has ${plural(kick.subscriptions, { one: "# subscription", other: "# subscriptions" })}, the chat service recorded ${kick.recorded} (the next reconcile fixes this)`,
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
        t`No chat instance is following YouTube chats (is YouTube configured on the chat service?)`,
      );
    }
    const last = youtube.last_error;
    const age = last ? now - Date.parse(last.at) : Number.POSITIVE_INFINITY;
    if (last?.quota && age < 24 * 3_600_000) {
      error(t`YouTube’s daily quota ran out (${last.message}); it resets at midnight Pacific time`);
    } else if (last && age < 3_600_000) {
      warning(`YouTube: ${last.message}`);
    }
  }

  if (chat.revoked.length > 0) {
    warning(
      plural(chat.revoked.length, {
        one: "# chat grant revoked: it needs connecting again",
        other: "# chat grants revoked: they need connecting again",
      }),
    );
  }
  return problems.sort((a, b) => (a.severity === b.severity ? 0 : a.severity === "error" ? -1 : 1));
}

/** Which connected switchers Admin → Status lists. */
export type ConnectedFilter = "all" | "live" | "obs" | "no-obs";

export const connectedFilters = (): { value: ConnectedFilter; label: string }[] => [
  { value: "all", label: t`All` },
  { value: "live", label: t`Live` },
  { value: "obs", label: t`OBS connected` },
  { value: "no-obs", label: t`No OBS` },
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

const adminActions = (target: string): Record<string, string> => ({
  suspend_workspace: t`Suspended ${target}`,
  lift_suspension: t`Lifted the suspension of ${target}`,
  set_plan: t`Set the plan of ${target}`,
  break_glass: t`Opened emergency access to ${target}`,
  reconnect_switcher: t`Reconnected a switcher in ${target}`,
  unlink_chat_channel: t`Unlinked a chat channel from ${target}`,
  sign_out_everywhere: t`Signed out everywhere: ${target}`,
  disable_user: t`Disabled ${target}`,
  enable_user: t`Enabled ${target}`,
  set_staff: t`Changed staff access of ${target}`,
  create_notice: t`Posted a notice`,
  update_notice: t`Changed a notice`,
  end_notice: t`Ended a notice`,
  delete_notice: t`Deleted a notice`,
  create_organization: t`Created ${target}`,
  update_organization: t`Changed the limits of ${target}`,
  delete_organization: t`Deleted ${target}`,
  suspend_organization: t`Suspended ${target}`,
  lift_organization_suspension: t`Lifted the suspension of ${target}`,
  invite_to_organization: t`Invited someone to the team of ${target}`,
});

/** What an audit entry says staff did, in words. */
export function describeAudit(
  entry: Pick<
    AdminAuditEntry,
    "kind" | "action" | "method" | "workspace_name" | "target_user_name" | "target_user_id"
  >,
): string {
  if (entry.kind === "support") {
    const what = describeSupportAccess({ method: entry.method ?? "GET", route: entry.action });
    return entry.workspace_name ? t`${what} in ${entry.workspace_name}` : what;
  }
  const target =
    entry.workspace_name ??
    entry.target_user_name ??
    (entry.target_user_id ? t`a deleted user` : "");
  return (adminActions(target)[entry.action] ?? `${entry.action} ${target}`).trim();
}
