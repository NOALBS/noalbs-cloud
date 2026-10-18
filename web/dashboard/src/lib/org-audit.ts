import type { OrganizationAuditEntry } from "@/api/types";
import { ORGANIZATION_ROLE_LABELS } from "@/lib/organizations";

type Details = Record<string, unknown>;

const text = (value: unknown): string | null => (typeof value === "string" && value ? value : null);
const count = (value: unknown): number => (typeof value === "number" ? value : 0);
const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

function allocation(value: unknown): string {
  return typeof value === "number" ? String(value) : "no cap";
}

function role(value: unknown): string {
  return value === "admin" || value === "member" ? ORGANIZATION_ROLE_LABELS[value] : String(value);
}

/** Who did it, for a line: a customer is said to be one. */
export function actorLabel(entry: Pick<OrganizationAuditEntry, "actor" | "actor_name">): string {
  return entry.actor === "customer" ? `${entry.actor_name} (customer)` : entry.actor_name;
}

/**
 * What an audit entry says, as a title and a detail line. Unknown actions (from a
 * newer API) still read as their code.
 */
export function describeAudit(entry: OrganizationAuditEntry): { title: string; detail: string } {
  const details = entry.details as Details;
  const subject = entry.subject ?? "a client";
  const client = text(details.client);
  const where = client ? `In ${client}` : "";

  switch (entry.action) {
    case "client_created":
      return {
        title: `Created ${subject}`,
        detail: `/${text(details.url_slug) ?? ""} · ${allocation(details.switcher_allocation)} switchers`,
      };
    case "client_updated": {
      const parts: string[] = [];
      const previousName = text(details.previous_name);
      if (previousName && previousName !== entry.subject)
        parts.push(`renamed from ${previousName}`);
      if (details.previous_allocation !== details.switcher_allocation) {
        parts.push(
          `allocation ${allocation(details.previous_allocation)} → ${allocation(details.switcher_allocation)}`,
        );
      }
      return { title: `Changed ${subject}`, detail: parts.join(" · ") || "No change" };
    }
    case "client_renamed":
      return {
        title: `Renamed ${text(details.previous_name) ?? "a client"} to ${subject}`,
        detail: "",
      };
    case "client_deleted":
      return { title: `Deleted ${subject}`, detail: `/${text(details.url_slug) ?? ""}` };
    case "clients_imported": {
      const created = count(details.created);
      const requested = count(details.requested);
      return {
        title: `Imported ${plural(created, "client")}`,
        detail: created < requested ? `${requested - created} could not be created` : "",
      };
    }
    case "client_invites_created": {
      const made = count(details.made);
      const requested = count(details.requested);
      return {
        title: `Made ${plural(made, "invite link")} for customers`,
        detail: made < requested ? `${requested - made} could not be made` : "",
      };
    }
    case "clients_allocation_changed":
      return {
        title: `Set the allocation of ${plural(count(details.changed), "client")} to ${allocation(details.switcher_allocation)}`,
        detail:
          count(details.changed) < count(details.requested)
            ? `${count(details.requested) - count(details.changed)} could not be changed`
            : "",
      };
    case "clients_deleted": {
      const names = Array.isArray(details.names) ? details.names.map(String) : [];
      const deleted = count(details.deleted);
      return {
        title: `Deleted ${plural(deleted, "client")}`,
        detail:
          names.length > 0
            ? `${names.join(", ")}${deleted > names.length ? ` and ${deleted - names.length} more` : ""}`
            : "",
      };
    }
    case "settings_changed":
      return {
        title: details.custom_chat_bot
          ? "Customers may connect their own chat bot"
          : "Customers can no longer use their own chat bot",
        detail: "",
      };
    case "chat_bot_connected":
      return {
        title: `Connected the ${text(details.platform) ?? ""} chat bot`,
        detail: text(details.account) ?? "",
      };
    case "chat_bot_removed":
      return { title: `Disconnected the ${text(details.platform) ?? ""} chat bot`, detail: "" };
    case "team_role_changed":
      return {
        title: `Made ${subject} ${role(details.role)}`,
        detail: `Was ${role(details.previous_role)}`,
      };
    case "team_member_removed":
      return { title: `Removed ${subject} from the team`, detail: "" };
    case "team_member_left":
      return { title: `${subject} left the team`, detail: "" };
    case "team_member_joined":
      return { title: "Joined the team", detail: "" };
    case "team_invite_created":
      return { title: `Made a team invite link (${role(details.role)})`, detail: "" };
    case "team_invite_revoked":
      return { title: "Revoked a team invite link", detail: "" };
    case "customer_invite_created":
      return { title: `Made a customer invite link for ${subject}`, detail: "" };
    case "customer_invite_revoked":
      return { title: `Revoked a customer invite link for ${subject}`, detail: "" };
    case "customer_joined":
      return { title: `Joined ${subject}`, detail: "Through an invite link" };
    case "customer_role_changed":
      return { title: `Changed the role of ${subject}`, detail: where };
    case "customer_removed":
      return { title: `Removed ${subject}`, detail: where };
    case "customer_left":
      return { title: `${subject} left`, detail: where };
    // What NOALBS did to the organization.
    case "create_organization":
      return { title: "Organization created", detail: "" };
    case "update_organization":
      return {
        title: "Limits changed",
        detail: `${details.max_switchers ?? "?"} switchers, ${details.max_stream_servers ?? "?"} stream servers per switcher, ${details.max_members ?? "?"} customers per client, ${details.history_days ?? "?"} days of history`,
      };
    case "suspend_organization":
      return { title: "Suspended", detail: text(details.reason) ?? "" };
    case "lift_organization_suspension":
      return { title: "Suspension lifted", detail: "" };
    case "invite_to_organization":
      return { title: `Made a team invite link (${role(details.role)})`, detail: "" };
    default:
      return { title: entry.action.replaceAll("_", " "), detail: entry.subject ?? "" };
  }
}
