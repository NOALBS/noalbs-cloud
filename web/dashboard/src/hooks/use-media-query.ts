import { useSyncExternalStore } from "react";

/** Whether a CSS media query matches, updating when it changes. */
export function useMediaQuery(query: string): boolean {
  return useSyncExternalStore(
    (onChange) => {
      const list = window.matchMedia(query);
      list.addEventListener("change", onChange);
      return () => list.removeEventListener("change", onChange);
    },
    () => window.matchMedia(query).matches,
  );
}

/** Tailwind's `lg` breakpoint, where pages switch to their desktop layout. */
export const DESKTOP = "(min-width: 64rem)";
