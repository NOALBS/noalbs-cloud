import { useCallback, useState } from "react";

/** Where a list is kept: `noalbs-<list>:<switcher id>`. */
type ListName = "hidden-scenes" | "hidden-sources" | "hidden-audio";
const storageKey = (list: ListName, switcherId: string) => `noalbs-${list}:${switcherId}`;

/**
 * A scene's (or source's) identity for hiding: its UUID, or its name on OBS
 * versions without UUIDs. Names are unique across a scene collection in OBS.
 */
export function sceneKey(scene: { uuid?: string | null; name: string }): string {
  return scene.uuid ?? `name:${scene.name}`;
}

/** Reads the stored list, tolerating missing or malformed values. */
export function parseHidden(raw: string | null): string[] {
  if (!raw) return [];
  try {
    const value: unknown = JSON.parse(raw);
    return Array.isArray(value) ? value.filter((item) => typeof item === "string") : [];
  } catch {
    return [];
  }
}

function read(list: ListName, switcherId: string): string[] {
  try {
    return parseHidden(localStorage.getItem(storageKey(list, switcherId)));
  } catch {
    return [];
  }
}

/**
 * The scenes hidden from a switcher's Live grid. Kept in this browser only, and
 * stored as the hidden ones so scenes added in OBS later show up by default.
 */
export function useHiddenScenes(switcherId: string) {
  return useHiddenList("hidden-scenes", switcherId);
}

/** The sources removed from a switcher's Live page, like `useHiddenScenes`. */
export function useHiddenSources(switcherId: string) {
  return useHiddenList("hidden-sources", switcherId);
}

/** The inputs removed from a switcher's Audio card, like `useHiddenSources`. */
export function useHiddenAudio(switcherId: string) {
  return useHiddenList("hidden-audio", switcherId);
}

function useHiddenList(list: ListName, switcherId: string) {
  const [state, setState] = useState(() => ({ switcherId, hidden: read(list, switcherId) }));
  // Re-read when the page moves to another switcher.
  const hidden = state.switcherId === switcherId ? state.hidden : read(list, switcherId);

  const setHidden = useCallback(
    (next: string[]) => {
      setState({ switcherId, hidden: next });
      try {
        if (next.length === 0) localStorage.removeItem(storageKey(list, switcherId));
        else localStorage.setItem(storageKey(list, switcherId), JSON.stringify(next));
      } catch {
        // Storage can be unavailable (private mode); the choice then lasts this visit.
      }
    },
    [list, switcherId],
  );

  return [hidden, setHidden] as const;
}
