import type { Me, OrganizationRole, SupportLevel, Workspace, WorkspaceRole } from "@/api/types";

/**
 * Mirrors the API's role rules so the UI can hide or disable what a user can't
 * do. The API enforces them; this is only for presentation.
 */
export type Permission =
  | "account.view"
  | "account.manage"
  | "switcher.view"
  | "switcher.control"
  | "switcher.manage"
  | "billing.manage"
  | "workspace.delete"
  | "workspace.transfer"
  | "workspace.support_access";

const ROLE_PERMISSIONS: Record<WorkspaceRole, readonly Permission[]> = {
  operator: ["account.view", "switcher.view", "switcher.control"],
  admin: [
    "account.view",
    "account.manage",
    "switcher.view",
    "switcher.control",
    "switcher.manage",
    "workspace.support_access",
  ],
  owner: [
    "account.view",
    "account.manage",
    "switcher.view",
    "switcher.control",
    "switcher.manage",
    "billing.manage",
    "workspace.delete",
    "workspace.transfer",
    "workspace.support_access",
  ],
};

/**
 * What the team of the organization that owns a client workspace may do there,
 * by their role in the organization (the API's `organization_role_permissions`).
 * Never billing or a transfer: there is nothing to pay for or hand over.
 */
const ORGANIZATION_PERMISSIONS: Record<OrganizationRole, readonly Permission[]> = {
  member: [
    "account.view",
    "account.manage",
    "switcher.view",
    "switcher.control",
    "switcher.manage",
  ],
  admin: [
    "account.view",
    "account.manage",
    "switcher.view",
    "switcher.control",
    "switcher.manage",
    "workspace.delete",
    "workspace.support_access",
  ],
};

/**
 * Who is acting: a member with a role, a member of the organization that owns
 * the (client) workspace, or staff (who may not be a member). For staff outside
 * the workspace, `support` is what its support grant allows (`null` = nothing).
 */
export interface Actor {
  role: WorkspaceRole | null;
  staff: boolean;
  support?: SupportLevel | null;
  /** The actor's role on the team of the organization that owns this client workspace. */
  organization?: OrganizationRole | null;
}

const SUPPORT_PERMISSIONS: Record<SupportLevel, readonly Permission[]> = {
  view: ["account.view", "switcher.view"],
  control: ["account.view", "switcher.view", "switcher.control", "switcher.manage"],
};

/** What a support grant covers; never members, billing or the workspace itself (as the API). */
export function supportAllows(level: SupportLevel, permission: Permission): boolean {
  return SUPPORT_PERMISSIONS[level].includes(permission);
}

export function can(actor: Actor, permission: Permission): boolean {
  if (actor.role !== null && ROLE_PERMISSIONS[actor.role].includes(permission)) return true;
  if (actor.organization && ORGANIZATION_PERMISSIONS[actor.organization].includes(permission)) {
    return true;
  }
  return actor.staff && actor.support != null && supportAllows(actor.support, permission);
}

/**
 * Staff in a workspace they aren't a member of: NOALBS support. The API logs
 * everything they do there for the workspace and never shows them secrets.
 */
export function isSupport(actor: Actor): boolean {
  return actor.staff && actor.role === null && !actor.organization;
}

/** Whether secrets (raw stats, the Moblin password) are shown: members who manage switchers. Stats URLs follow the API. */
export function canReadSecrets(actor: Actor): boolean {
  if (actor.organization)
    return ORGANIZATION_PERMISSIONS[actor.organization].includes("switcher.manage");
  return actor.role !== null && ROLE_PERMISSIONS[actor.role].includes("switcher.manage");
}

const isOwnerOrStaff = (actor: Actor) =>
  actor.staff || actor.role === "owner" || Boolean(actor.organization);

/** The roles `actor` may give a member who currently has `current`. */
export function assignableRoles(actor: Actor, current: WorkspaceRole): WorkspaceRole[] {
  if (current === "owner" || !can(actor, "account.manage")) return [];
  if (isOwnerOrStaff(actor)) return ["operator", "admin"];
  return current === "admin" ? [] : ["operator"];
}

/** The roles `actor` may invite people as. */
export function invitableRoles(actor: Actor): WorkspaceRole[] {
  if (!can(actor, "account.manage")) return [];
  return isOwnerOrStaff(actor) ? ["operator", "admin"] : ["operator"];
}

export function canRemove(actor: Actor, removingSelf: boolean, target: WorkspaceRole): boolean {
  if (target === "owner") return false;
  if (removingSelf) return true;
  if (!can(actor, "account.manage")) return false;
  if (isOwnerOrStaff(actor)) return true;
  return actor.role === "admin" && target === "operator";
}

export const ROLE_LABELS: Record<WorkspaceRole, string> = {
  operator: "Operator",
  admin: "Admin",
  owner: "Owner",
};

export const ROLE_DESCRIPTIONS: Record<WorkspaceRole, string> = {
  operator: "Runs the stream: switch scenes, start and stop, see stats.",
  admin: "Also manages switchers, stream servers, members and invites.",
  owner: "Also handles the plan and billing. One per workspace.",
};

/** The members `actor` may make the owner: an admin, or anyone but the owner for staff. */
export function transferCandidates<M extends { role: WorkspaceRole }>(
  actor: Actor,
  members: M[],
): M[] {
  if (!can(actor, "workspace.transfer")) return [];
  return members.filter((member) =>
    actor.staff ? member.role !== "owner" : member.role === "admin",
  );
}

/** The actor for `me` in `workspace`. */
export function actorOf(
  workspace: Pick<Workspace, "role" | "support_access"> & {
    organization_role?: OrganizationRole | null;
  },
  me: Pick<Me, "staff">,
): Actor {
  return {
    role: workspace.role ?? null,
    staff: me.staff,
    support: workspace.support_access?.level ?? null,
    organization: workspace.organization_role ?? null,
  };
}
