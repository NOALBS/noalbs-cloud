const KEY = "noalbs-recent-workspaces";

/** How many workspaces a menu lists before pointing at "All workspaces". */
export const MENU_WORKSPACES = 8;

/** The workspaces opened last in this browser, newest first. */
export function recentWorkspaceIds(): string[] {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(parsed) ? parsed.filter((id): id is string => typeof id === "string") : [];
  } catch {
    return [];
  }
}

/** Puts a workspace at the front of the recent ones (kept in this browser only). */
export function rememberWorkspace(workspaceId: string) {
  try {
    const recent = [workspaceId, ...recentWorkspaceIds().filter((id) => id !== workspaceId)];
    localStorage.setItem(KEY, JSON.stringify(recent.slice(0, 30)));
  } catch {
    // Storage can be unavailable (private mode); this is only a convenience.
  }
}

/**
 * What a workspace menu lists: all of them when there are few, else the current one
 * and the ones opened most recently, at most `max`. The rest are one click away on
 * the All workspaces page, which searches.
 */
export function menuWorkspaces<T extends { id: string }>(
  workspaces: T[],
  currentId: string | null,
  recent: string[],
  max = MENU_WORKSPACES,
): { shown: T[]; hidden: number } {
  if (workspaces.length <= max) return { shown: workspaces, hidden: 0 };

  const byId = new Map(workspaces.map((workspace) => [workspace.id, workspace]));
  const order = [...(currentId ? [currentId] : []), ...recent];
  const shown: T[] = [];
  for (const id of order) {
    const workspace = byId.get(id);
    if (workspace && !shown.includes(workspace)) shown.push(workspace);
    if (shown.length === max) break;
  }
  // Someone with no history yet gets the first few, so the menu is never empty.
  for (const workspace of workspaces) {
    if (shown.length >= Math.min(max, 3) && shown.length > 0) break;
    if (!shown.includes(workspace)) shown.push(workspace);
  }
  return { shown, hidden: workspaces.length - shown.length };
}
