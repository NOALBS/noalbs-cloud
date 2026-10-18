import { describe, expect, it } from "vitest";
import {
  assignableRoles,
  can,
  canReadSecrets,
  canRemove,
  invitableRoles,
  isSupport,
  transferCandidates,
} from "./permissions";

const owner = { role: "owner", staff: false } as const;
const admin = { role: "admin", staff: false } as const;
const operator = { role: "operator", staff: false } as const;
const staff = { role: null, staff: true } as const;

describe("can", () => {
  it("follows the role table", () => {
    expect(can(operator, "switcher.control")).toBe(true);
    expect(can(operator, "account.manage")).toBe(false);
    expect(can(admin, "switcher.manage")).toBe(true);
    expect(can(admin, "billing.manage")).toBe(false);
    expect(can(owner, "billing.manage")).toBe(true);
    expect(can({ role: "admin", staff: true }, "billing.manage")).toBe(false);
    expect(can({ role: null, staff: false }, "account.view")).toBe(false);
  });
});

describe("member rules", () => {
  it("only lets the owner manage admins", () => {
    expect(assignableRoles(owner, "operator")).toEqual(["operator", "admin"]);
    expect(assignableRoles(admin, "operator")).toEqual(["operator"]);
    expect(assignableRoles(admin, "admin")).toEqual([]);
    expect(assignableRoles(owner, "owner")).toEqual([]);
    expect(assignableRoles(operator, "operator")).toEqual([]);
    expect(invitableRoles(admin)).toEqual(["operator"]);
    expect(invitableRoles({ role: "admin", staff: true })).toEqual(["operator", "admin"]);
    expect(invitableRoles({ ...staff, support: "control" })).toEqual([]);
  });

  it("lets anyone but the owner leave", () => {
    expect(canRemove(operator, true, "operator")).toBe(true);
    expect(canRemove(owner, true, "owner")).toBe(false);
    expect(canRemove(admin, false, "operator")).toBe(true);
    expect(canRemove(admin, false, "admin")).toBe(false);
    expect(canRemove(operator, false, "operator")).toBe(false);
  });
});

describe("transferCandidates", () => {
  const members = [
    { id: "o", role: "owner" as const },
    { id: "a", role: "admin" as const },
    { id: "p", role: "operator" as const },
  ];
  const ids = (list: { id: string }[]) => list.map((member) => member.id);

  it("lets the owner pick an admin and staff anyone but the owner", () => {
    expect(ids(transferCandidates({ role: "owner", staff: false }, members))).toEqual(["a"]);
    expect(ids(transferCandidates({ role: "owner", staff: true }, members))).toEqual(["a", "p"]);
    // Support never hands a workspace over.
    expect(transferCandidates({ role: null, staff: true, support: "control" }, members)).toEqual(
      [],
    );
    expect(transferCandidates({ role: "admin", staff: false }, members)).toEqual([]);
  });
});

describe("support", () => {
  it("is staff outside their own workspaces", () => {
    expect(isSupport(staff)).toBe(true);
    expect(isSupport({ role: "admin", staff: true })).toBe(false);
    expect(isSupport(owner)).toBe(false);
  });

  it("never gets secrets, which only managing members see", () => {
    expect(canReadSecrets(staff)).toBe(false);
    expect(canReadSecrets(admin)).toBe(true);
    expect(canReadSecrets(operator)).toBe(false);
  });
});

describe("support grants", () => {
  it("give staff only what the workspace allowed", () => {
    expect(can(staff, "switcher.view")).toBe(false);
    expect(can({ ...staff, support: "view" }, "switcher.view")).toBe(true);
    expect(can({ ...staff, support: "view" }, "switcher.control")).toBe(false);
    expect(can({ ...staff, support: "control" }, "switcher.manage")).toBe(true);
    for (const never of ["account.manage", "billing.manage", "workspace.support_access"] as const) {
      expect(can({ ...staff, support: "control" }, never)).toBe(false);
    }
    expect(canRemove({ ...staff, support: "control" }, false, "operator")).toBe(false);
  });
});

describe("organization team", () => {
  const orgAdmin = { role: null, staff: false, organization: "admin" } as const;
  const orgMember = { role: null, staff: false, organization: "member" } as const;

  it("runs a client workspace but never bills or hands it over", () => {
    for (const actor of [orgAdmin, orgMember]) {
      expect(can(actor, "switcher.manage")).toBe(true);
      expect(can(actor, "account.manage")).toBe(true);
      expect(can(actor, "billing.manage")).toBe(false);
      expect(can(actor, "workspace.transfer")).toBe(false);
      expect(canReadSecrets(actor)).toBe(true);
    }
    expect(can(orgAdmin, "workspace.delete")).toBe(true);
    expect(can(orgMember, "workspace.delete")).toBe(false);
  });

  it("is not support, even for staff", () => {
    expect(isSupport({ ...orgAdmin, staff: true })).toBe(false);
    expect(isSupport(staff)).toBe(true);
  });

  it("hands out admin and removes anyone, like the owner", () => {
    expect(assignableRoles(orgMember, "operator")).toEqual(["operator", "admin"]);
    expect(invitableRoles(orgAdmin)).toEqual(["operator", "admin"]);
    expect(canRemove(orgMember, false, "admin")).toBe(true);
    expect(canRemove(orgMember, false, "owner")).toBe(false);
  });
});
