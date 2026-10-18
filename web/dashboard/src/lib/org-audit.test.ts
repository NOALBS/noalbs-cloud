import { describe, expect, it } from "vitest";
import type { OrganizationAuditEntry } from "@/api/types";
import { actorLabel, describeAudit } from "./org-audit";

const entry = (
  action: string,
  subject: string | null,
  details: Record<string, unknown> = {},
  actor: OrganizationAuditEntry["actor"] = "team",
): OrganizationAuditEntry => ({
  id: "e1",
  created_at: "2026-10-16T12:00:00Z",
  actor_name: "kate",
  actor,
  action,
  subject,
  details: details as OrganizationAuditEntry["details"],
});

describe("describeAudit", () => {
  it("says what changed on a client", () => {
    expect(
      describeAudit(
        entry("client_updated", "Nomad Live", {
          previous_name: "Nomad",
          switcher_allocation: 2,
          previous_allocation: null,
        }),
      ),
    ).toEqual({
      title: "Changed Nomad Live",
      detail: "renamed from Nomad · allocation no cap → 2",
    });
    expect(
      describeAudit(
        entry("client_updated", "Nomad", {
          previous_name: "Nomad",
          switcher_allocation: 1,
          previous_allocation: 1,
        }),
      ).detail,
    ).toBe("No change");
  });

  it("counts bulk actions and names what was deleted", () => {
    expect(describeAudit(entry("clients_imported", null, { requested: 5, created: 4 }))).toEqual({
      title: "Imported 4 clients",
      detail: "1 could not be created",
    });
    expect(
      describeAudit(
        entry("clients_deleted", null, { requested: 3, deleted: 3, names: ["A", "B"] }),
      ),
    ).toEqual({ title: "Deleted 3 clients", detail: "A, B and 1 more" });
    expect(
      describeAudit(entry("client_invites_created", null, { requested: 1, made: 1 })).title,
    ).toBe("Made 1 invite link for customers");
  });

  it("reads team and customer actions", () => {
    expect(
      describeAudit(entry("team_role_changed", "sam", { role: "admin", previous_role: "member" })),
    ).toEqual({ title: "Made sam Admin", detail: "Was Member" });
    expect(describeAudit(entry("settings_changed", null, { custom_chat_bot: true })).title).toBe(
      "Customers may connect their own chat bot",
    );
    expect(describeAudit(entry("customer_joined", "Nomad Live", {}, "customer")).title).toBe(
      "Joined Nomad Live",
    );
    expect(
      describeAudit(entry("customer_removed", "alex", { client: "Nomad Live" }, "team")).detail,
    ).toBe("In Nomad Live");
  });

  it("reads what NOALBS did and keeps an unknown action readable", () => {
    expect(
      describeAudit(entry("suspend_organization", null, { reason: "unpaid" }, "staff")),
    ).toEqual({ title: "Suspended", detail: "unpaid" });
    expect(describeAudit(entry("some_new_thing", "x")).title).toBe("some new thing");
  });
});

describe("actorLabel", () => {
  it("marks a customer", () => {
    expect(actorLabel({ actor: "customer", actor_name: "kate" })).toBe("kate (customer)");
    expect(actorLabel({ actor: "team", actor_name: "kate" })).toBe("kate");
  });
});
