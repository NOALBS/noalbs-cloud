import type { WorkspaceRole } from "@/api/types";
import { CLIENT_STATES, type ClientState } from "@/lib/organizations";

/** The roles a workspace list can narrow to, as the API takes them. */
export const MEMBER_ROLES: WorkspaceRole[] = ["owner", "admin", "operator"];

/**
 * The all-workspaces page's address: a search, a role, what the workspace is doing,
 * an order and the page, read from the URL.
 */
export interface WorkspacesSearch {
  q?: string;
  page?: number;
  role?: WorkspaceRole;
  state?: ClientState;
  /** Oldest first when left out. */
  sort?: "name";
}

/** Reads `WorkspacesSearch` from an untrusted query string, dropping what doesn't fit. */
export function parseWorkspacesSearch(search: Record<string, unknown>): WorkspacesSearch {
  const page = Number(search.page);
  return {
    q: typeof search.q === "string" && search.q ? search.q : undefined,
    page: Number.isInteger(page) && page > 1 ? page : undefined,
    role: MEMBER_ROLES.find((role) => role === search.role),
    state: CLIENT_STATES.find((state) => state === search.state),
    sort: search.sort === "name" ? "name" : undefined,
  };
}
