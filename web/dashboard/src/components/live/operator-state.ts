import type { SwitcherEvent } from "@/api/types";

export type Scene = { uuid: string; name: string };
export const transitions = ["Cut", "Fade", "Stinger", "Fade to black"] as const;
export type OperatorState = {
  studio: boolean;
  preview: Scene | null;
  transition: (typeof transitions)[number];
  duration: number;
};
export const operatorKey = (id: string) => `noalbs-operator:v1:${id}`;
export const initialOperator: OperatorState = {
  studio: true,
  preview: null,
  transition: "Fade",
  duration: 300,
};
export function readOperator(id: string): OperatorState {
  try {
    const value = JSON.parse(localStorage.getItem(operatorKey(id)) ?? "null");
    if (!value) return initialOperator;
    return {
      studio: typeof value.studio === "boolean" ? value.studio : true,
      preview:
        value.preview &&
        typeof value.preview.uuid === "string" &&
        typeof value.preview.name === "string"
          ? value.preview
          : null,
      transition: transitions.includes(value.transition) ? value.transition : "Fade",
      duration: Number.isFinite(value.duration)
        ? Math.max(0, Math.min(10000, value.duration))
        : 300,
    };
  } catch {
    return initialOperator;
  }
}

export function swapAfterTake(state: OperatorState, previous: Scene | null): OperatorState {
  return { ...state, preview: previous };
}

export function operatorHotkey(
  event: Pick<
    KeyboardEvent,
    "key" | "code" | "shiftKey" | "altKey" | "metaKey" | "ctrlKey" | "repeat" | "isComposing"
  >,
  context: { desktop: boolean; editing: boolean; blocked: boolean; button: boolean },
):
  | { action: "scene"; index: number; direct: boolean }
  | { action: "take" | "cut" | "help" | "immersive" }
  | null {
  if (
    !context.desktop ||
    context.editing ||
    context.blocked ||
    event.repeat ||
    event.isComposing ||
    event.altKey ||
    event.metaKey ||
    event.ctrlKey
  )
    return null;
  if (event.key === "?") return { action: "help" };
  if (event.key.toLowerCase() === "f") return { action: "immersive" };
  const digit =
    /^(?:Digit|Numpad)([1-9])$/.exec(event.code)?.[1] ??
    (/^[1-9]$/.test(event.key) ? event.key : null);
  if (digit) return { action: "scene", index: Number(digit) - 1, direct: event.shiftKey };
  if (context.button || event.shiftKey) return null;
  if (event.key === " ") return { action: "take" };
  if (event.key === "Enter") return { action: "cut" };
  return null;
}

export function switchedBy(events: SwitcherEvent[], program?: string | null): string | null {
  const event = [...events]
    .sort((a, b) => Date.parse(b.created_at) - Date.parse(a.created_at))
    .find((e) => e.kind === "scene_switched");
  if (event?.kind !== "scene_switched" || event.data.to !== program) return null;
  const { by, reason, bitrate } = event.data;
  if (by) return by;
  if (reason === "obs") return "Changed in OBS";
  if (reason === "dashboard") return "dashboard";
  return `auto switch${bitrate != null ? ` · ${bitrate.toLocaleString()} kbps` : ""}${event.data.server ? ` · ${event.data.server}` : ""}`;
}
