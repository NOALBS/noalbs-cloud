import { useSyncExternalStore } from "react";

/** Whether the tab is visible, updating when it's hidden or shown. */
export function useDocumentVisible(): boolean {
  return useSyncExternalStore(
    (onChange) => {
      document.addEventListener("visibilitychange", onChange);
      return () => document.removeEventListener("visibilitychange", onChange);
    },
    () => !document.hidden,
  );
}
