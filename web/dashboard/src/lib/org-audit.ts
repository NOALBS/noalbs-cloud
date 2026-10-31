import { plural, t } from "@lingui/core/macro";
import type { OrganizationAuditEntry } from "@/api/types";
import { organizationRoleLabels } from "@/lib/organizations";

type Details = Record<string, unknown>;

const text = (value: unknown): string | null => (typeof value === "string" && value ? value : null);
const count = (value: unknown): number => (typeof value === "number" ? value : 0);
const limit = (value: unknown): string => String(value ?? "?");

function allocation(value: unknown): string {
  return typeof value === "number" ? String(value) : t`no cap`;
}

function role(value: unknown): string {
  return value === "admin" || value === "member" ? organizationRoleLabels()[value] : String(value);
}

/** Who did it, for a line: a customer is said to be one. */
export function actorLabel(entry: Pick<OrganizationAuditEntry, "actor" | "actor_name">): string {
  return entry.actor === "customer" ? t`${entry.actor_name} (customer)` : entry.actor_name;
}

/**
 * What an audit entry says, as a title and a detail line. Unknown actions (from a
 * newer API) still read as their code.
 */
export function describeAudit(entry: OrganizationAuditEntry): { title: string; detail: string } {
  const details = entry.details as Details;
  const subject = entry.subject ?? t`a client`;
  const client = text(details.client);
  const where = client ? t`In ${client}` : "";

  switch (entry.action) {
    case "client_created":
      return {
        title: t`Created ${subject}`,
        detail: `/${text(details.url_slug) ?? ""} · ${t`${allocation(details.switcher_allocation)} switchers`}`,
      };
    case "client_updated": {
      const parts: string[] = [];
      const previousName = text(details.previous_name);
      if (previousName && previousName !== entry.subject)
        parts.push(t`renamed from ${previousName}`);
      if (details.previous_allocation !== details.switcher_allocation) {
        parts.push(
          t`allocation ${allocation(details.previous_allocation)} → ${allocation(details.switcher_allocation)}`,
        );
      }
      return { title: t`Changed ${subject}`, detail: parts.join(" · ") || t`No change` };
    }
    case "client_renamed":
      return {
        title: t`Renamed ${text(details.previous_name) ?? t`a client`} to ${subject}`,
        detail: "",
      };
    case "client_deleted":
      return { title: t`Deleted ${subject}`, detail: `/${text(details.url_slug) ?? ""}` };
    case "clients_imported": {
      const created = count(details.created);
      const requested = count(details.requested);
      return {
        title: plural(created, { one: "Imported # client", other: "Imported # clients" }),
        detail: created < requested ? t`${requested - created} could not be created` : "",
      };
    }
    case "client_invites_created": {
      const made = count(details.made);
      const requested = count(details.requested);
      return {
        title: plural(made, {
          one: "Made # invite link for customers",
          other: "Made # invite links for customers",
        }),
        detail: made < requested ? t`${requested - made} could not be made` : "",
      };
    }
    case "clients_allocation_changed":
      return {
        title: t`Set the allocation of ${plural(count(details.changed), { one: "# client", other: "# clients" })} to ${allocation(details.switcher_allocation)}`,
        detail:
          count(details.changed) < count(details.requested)
            ? t`${count(details.requested) - count(details.changed)} could not be changed`
            : "",
      };
    case "clients_deleted": {
      const names = Array.isArray(details.names) ? details.names.map(String) : [];
      const deleted = count(details.deleted);
      return {
        title: plural(deleted, { one: "Deleted # client", other: "Deleted # clients" }),
        detail:
          names.length > 0
            ? deleted > names.length
              ? t`${names.join(", ")} and ${deleted - names.length} more`
              : names.join(", ")
            : "",
      };
    }
    case "settings_changed":
      return {
        title: details.custom_chat_bot
          ? t`Customers may connect their own chat bot`
          : t`Customers can no longer use their own chat bot`,
        detail: "",
      };
    case "chat_bot_connected":
      return {
        title: t`Connected the ${text(details.platform) ?? ""} chat bot`,
        detail: text(details.account) ?? "",
      };
    case "chat_bot_removed":
      return { title: t`Disconnected the ${text(details.platform) ?? ""} chat bot`, detail: "" };
    case "team_role_changed":
      return {
        title: t`Made ${subject} ${role(details.role)}`,
        detail: t`Was ${role(details.previous_role)}`,
      };
    case "team_member_removed":
      return { title: t`Removed ${subject} from the team`, detail: "" };
    case "team_member_left":
      return { title: t`${subject} left the team`, detail: "" };
    case "team_member_joined":
      return { title: t`Joined the team`, detail: "" };
    case "team_invite_created":
      return { title: t`Made a team invite link (${role(details.role)})`, detail: "" };
    case "team_invite_revoked":
      return { title: t`Revoked a team invite link`, detail: "" };
    case "customer_invite_created":
      return { title: t`Made a customer invite link for ${subject}`, detail: "" };
    case "customer_invite_revoked":
      return { title: t`Revoked a customer invite link for ${subject}`, detail: "" };
    case "customer_joined":
      return { title: t`Joined ${subject}`, detail: t`Through an invite link` };
    case "customer_role_changed":
      return { title: t`Changed the role of ${subject}`, detail: where };
    case "customer_removed":
      return { title: t`Removed ${subject}`, detail: where };
    case "customer_left":
      return { title: t`${subject} left`, detail: where };
    // What NOALBS did to the organization.
    case "create_organization":
      return { title: t`Organization created`, detail: "" };
    case "update_organization":
      return {
        title: t`Limits changed`,
        detail: t`${limit(details.max_switchers)} switchers, ${limit(details.max_stream_servers)} stream servers per switcher, ${limit(details.max_members)} customers per client, ${limit(details.history_days)} days of history`,
      };
    case "suspend_organization":
      return { title: t`Suspended`, detail: text(details.reason) ?? "" };
    case "lift_organization_suspension":
      return { title: t`Suspension lifted`, detail: "" };
    case "invite_to_organization":
      return { title: t`Made a team invite link (${role(details.role)})`, detail: "" };
    default:
      return { title: entry.action.replaceAll("_", " "), detail: entry.subject ?? "" };
  }
}
