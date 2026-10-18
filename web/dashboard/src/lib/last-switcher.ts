const key = (workspaceId: string) => `noalbs-last-switcher:${workspaceId}`;

/** The switcher last opened in a workspace, remembered in this browser only. */
export function lastSwitcherId(workspaceId: string): string | null {
  try {
    return localStorage.getItem(key(workspaceId));
  } catch {
    return null;
  }
}

export function rememberSwitcher(workspaceId: string, switcherId: string) {
  try {
    localStorage.setItem(key(workspaceId), switcherId);
  } catch {
    // Storage can be unavailable (private mode); this is only a convenience.
  }
}
