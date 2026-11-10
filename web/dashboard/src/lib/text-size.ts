import { useSyncExternalStore } from "react";

export type TextSize = "default" | "large" | "larger";

const STORAGE_KEY = "noalbs-text-size";
const listeners = new Set<() => void>();

export function parseTextSize(saved: string | null): TextSize {
  return saved === "large" || saved === "larger" ? saved : "default";
}

function currentTextSize(): TextSize {
  return parseTextSize(document.documentElement.dataset.textSize ?? null);
}

export function setTextSize(size: TextSize) {
  try {
    localStorage.setItem(STORAGE_KEY, size);
  } catch {
    // Storage can be unavailable (private mode); the choice then lasts this visit.
  }
  document.documentElement.dataset.textSize = size;
  for (const listener of listeners) listener();
}

const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => listeners.delete(listener);
};

export function useTextSize() {
  const size = useSyncExternalStore(subscribe, currentTextSize, () => "default" as const);
  return { size, setSize: setTextSize };
}
