import { useState } from "react";
import { compareItems, type OperatorItem } from "./model";

export interface OperatorPin {
  workspace: string;
  switcher: string;
}
export const PINS_KEY = "noalbs-operator-pins";
export function parsePins(value: string | null): OperatorPin[] {
  try {
    const data: unknown = JSON.parse(value ?? "[]");
    if (!Array.isArray(data)) return [];
    const pins: OperatorPin[] = [];
    for (const row of data) {
      if (
        row &&
        typeof row.workspace === "string" &&
        row.workspace &&
        typeof row.switcher === "string" &&
        row.switcher &&
        !pins.some((pin) => pin.switcher === row.switcher)
      )
        pins.push({ workspace: row.workspace, switcher: row.switcher });
      if (pins.length === 4) break;
    }
    return pins;
  } catch {
    return [];
  }
}
export const pinOf = (item: OperatorItem): OperatorPin => ({
  workspace: item.workspace_slug,
  switcher: item.switcher_id,
});
export function fillPins(pins: OperatorPin[], items: OperatorItem[]): OperatorPin[] {
  const next = parsePins(JSON.stringify(pins));
  for (const item of [...items].filter((item) => item.connected).sort(compareItems)) {
    if (next.length >= 4) break;
    if (!next.some((pin) => pin.switcher === item.switcher_id)) next.push(pinOf(item));
  }
  return next;
}
export function useOperatorPins() {
  const [pins, setPins] = useState(() => {
    try {
      return parsePins(localStorage.getItem(PINS_KEY));
    } catch {
      return [];
    }
  });
  const save = (next: OperatorPin[]) => {
    const valid = parsePins(JSON.stringify(next));
    setPins(valid);
    try {
      localStorage.setItem(PINS_KEY, JSON.stringify(valid));
    } catch {
      /* Storage may be unavailable. */
    }
  };
  return { pins, save };
}
