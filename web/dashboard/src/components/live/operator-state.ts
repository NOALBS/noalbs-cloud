import { t } from "@lingui/core/macro";
import type { SwitcherEvent } from "@/api/types";

export type Scene = { uuid: string; name: string };
export type OperatorState = {
  studio: boolean;
  preview: Scene | null;
  /** An OBS transition by name; null follows the one OBS has picked. */
  transition: string | null;
  /** Milliseconds, within what OBS accepts. */
  duration: number;
};
export const operatorKey = (id: string) => `noalbs-operator:v1:${id}`;
export const initialOperator: OperatorState = {
  studio: true,
  preview: null,
  transition: null,
  duration: 300,
};
export const clampDuration = (value: number) =>
  Number.isFinite(value) ? Math.max(50, Math.min(20000, Math.round(value))) : 300;
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
      transition: typeof value.transition === "string" ? value.transition : null,
      duration: clampDuration(value.duration),
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
  > & { target?: EventTarget | null },
  context: { desktop: boolean; editing: boolean; blocked: boolean; hold: boolean },
):
  | { action: "scene"; index: number; direct: boolean }
  | { action: "take" | "cut" | "help" | "immersive" | "search" }
  | null {
  if (
    !context.desktop ||
    context.editing ||
    context.blocked ||
    (event.repeat && event.key !== " " && event.key !== "Enter") ||
    event.isComposing ||
    event.altKey ||
    event.metaKey ||
    event.ctrlKey
  )
    return null;
  if (event.key === "?") return { action: "help" };
  if (event.key === "/") return { action: "search" };
  if (event.key.toLowerCase() === "f") return { action: "immersive" };
  const digit =
    /^(?:Digit|Numpad)([1-9])$/.exec(event.code)?.[1] ??
    (/^[1-9]$/.test(event.key) ? event.key : null);
  if (digit) return { action: "scene", index: Number(digit) - 1, direct: event.shiftKey };
  if (context.hold || event.shiftKey) return null;
  if (event.key === " " || event.key === "Enter") {
    const control =
      event.target instanceof Element
        ? event.target.closest(
            'button, a[href], input, textarea, select, summary, [role="button"], [role="switch"], [role="combobox"], [role="link"], [role="checkbox"], [role="radio"], [role="slider"], [role="spinbutton"], [role="menuitem"], [role="tab"], [role="option"], [role="treeitem"], [role="listbox"], [tabindex]',
          )
        : null;
    if (control && !control.matches("[data-scene-tile]")) return null;
  }
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
  if (reason === "obs") return t`Changed in OBS`;
  if (reason === "dashboard") return t`the dashboard`;
  return [
    t`auto switch`,
    ...(bitrate != null ? [`${bitrate.toLocaleString()} kbps`] : []),
    ...(event.data.server ? [event.data.server] : []),
  ].join(" · ");
}
