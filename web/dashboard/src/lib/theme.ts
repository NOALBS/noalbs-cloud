import { useSyncExternalStore } from "react";

/** The colours in use. */
export type Theme = "dark" | "light";
/** What the viewer picked: a theme, or `system` to follow the browser/OS setting. */
export type ThemePreference = Theme | "system";

/** The same key and values the pre-paint script in the HTML pages reads. */
const STORAGE_KEY = "noalbs-theme";
const listeners = new Set<() => void>();
// Kept for the page's lifetime: a MediaQueryList nobody holds can be collected
// along with its change listener.
const lightScheme =
  typeof window !== "undefined" && window.matchMedia
    ? window.matchMedia("(prefers-color-scheme: light)")
    : null;
const prefersLight = () => lightScheme?.matches ?? false;

/** The stored preference; anything else (nothing saved yet, or unreadable) is `system`. */
export function parsePreference(saved: string | null): ThemePreference {
  return saved === "light" || saved === "dark" ? saved : "system";
}

function readPreference(): ThemePreference {
  try {
    return parsePreference(localStorage.getItem(STORAGE_KEY));
  } catch {
    return "system";
  }
}

export function resolveTheme(preference: ThemePreference, systemLight: boolean): Theme {
  if (preference !== "system") return preference;
  return systemLight ? "light" : "dark";
}

function currentTheme(): Theme {
  return document.documentElement.dataset.theme === "light" ? "light" : "dark";
}

function apply(theme: Theme) {
  document.documentElement.dataset.theme = theme;
  document
    .querySelector('meta[name="theme-color"]')
    ?.setAttribute("content", theme === "light" ? "#F5F6F8" : "#0B0D10");
  for (const listener of listeners) listener();
}

export function setThemePreference(preference: ThemePreference) {
  try {
    localStorage.setItem(STORAGE_KEY, preference);
  } catch {
    // Storage can be unavailable (private mode); the choice then lasts this visit.
  }
  apply(resolveTheme(preference, prefersLight()));
}

// On Auto, follow the browser/OS when it changes (e.g. at sunset).
lightScheme?.addEventListener("change", (event) => {
  if (readPreference() === "system") apply(event.matches ? "light" : "dark");
});

const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => listeners.delete(listener);
};

/** The theme in use (set before first paint by the HTML page), and the viewer's preference. */
export function useTheme() {
  const theme = useSyncExternalStore(subscribe, currentTheme, () => "dark" as Theme);
  const preference = useSyncExternalStore(subscribe, readPreference, () => "system" as const);
  return { theme, preference, setPreference: setThemePreference };
}
