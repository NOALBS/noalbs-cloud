import { t } from "@lingui/core/macro";
export const paneDefinitions = [
  { id: "monitors", title: "Preview / Program", min: 620, operator: true },
  { id: "preview", title: "Preview", min: 300 },
  { id: "on-air", title: "On air", min: 220 },
  { id: "controls", title: "Actions", min: 300 },
  { id: "bitrate", title: "Bitrate", min: 300 },
  { id: "scenes", title: "Scenes", min: 320 },
  { id: "sources", title: "Sources", min: 280 },
  { id: "audio", title: "Audio", min: 300 },
  { id: "servers", title: "Stream servers", min: 320 },
  { id: "devices", title: "Devices", min: 340 },
  { id: "belabox", title: "BELABOX", min: 340 },
  { id: "moblin", title: "Moblin", min: 340 },
  { id: "obs", title: "OBS", min: 260 },
  { id: "activity", title: "Activity", min: 300 },
] as const;
export type PaneId = (typeof paneDefinitions)[number]["id"];
export type Mode = "streamer" | "operator";
export type Slot = { pane: PaneId | null; size: number };
export type Row = { slots: Slot[]; minHeight?: number };
export type Saved = { desktop: Row[]; phone: PaneId[] };
export type Address = { row: number; slot: number };
export type Target =
  | { kind: "row"; row: number }
  | { kind: "center" | "left" | "right"; row: number; slot: number };
export type Action =
  | { type: "add-row"; at: number; columns: number }
  | { type: "remove-row"; row: number }
  | { type: "remove-slot" | "beside"; row: number; slot: number }
  | { type: "place"; row: number; slot: number; pane: PaneId }
  | { type: "hide"; row: number; slot: number }
  | { type: "sizes"; row: number; left: number; right: number; sizes: [number, number] }
  | { type: "height"; row: number; value?: number }
  | { type: "drop"; source: Address; target: Target };
export const GAP = 16;
export const storageKey = (switcher: string, mode: Mode) =>
  `noalbs-live-rows:v1:${switcher}:${mode}`;
/** A pane's name in the reader's language; the definitions keep the English one. */
export const title = (pane: PaneId): string => {
  switch (pane) {
    case "monitors":
      return t`Preview / Program`;
    case "preview":
      return t`Preview`;
    case "on-air":
      return t`On air`;
    case "controls":
      return t`Actions`;
    case "bitrate":
      return t`Bitrate`;
    case "scenes":
      return t`Scenes`;
    case "sources":
      return t`Sources`;
    case "audio":
      return t`Audio`;
    case "servers":
      return t`Stream servers`;
    case "devices":
      return t`Devices`;
    case "obs":
      return t`OBS`;
    case "activity":
      return t`Activity`;
    default:
      return paneDefinitions.find((p) => p.id === pane)?.title ?? pane;
  }
};
export const minimum = (slot: Slot | undefined) =>
  paneDefinitions.find((p) => p.id === slot?.pane)?.min ?? 180;
const row = (...items: (PaneId | [PaneId, number])[]): Row => ({
  slots: items.map((item) =>
    typeof item === "string" ? { pane: item, size: 1 } : { pane: item[0], size: item[1] },
  ),
});
export function defaults(mode: Mode): Saved {
  return {
    desktop:
      mode === "streamer"
        ? [
            row(["preview", 3], ["on-air", 2], ["bitrate", 4]),
            row(["scenes", 5], ["audio", 3], ["servers", 3]),
            row("sources", "obs", "belabox", "moblin"),
            row("activity"),
          ]
        : [
            row(["monitors", 7], ["bitrate", 3]),
            row(["scenes", 7], ["servers", 3]),
            row("sources", "audio", "devices", "obs"),
            row("activity"),
          ],
    phone: [
      "bitrate",
      "controls",
      "scenes",
      "servers",
      "belabox",
      "moblin",
      "sources",
      "audio",
      "activity",
    ],
  };
}
const positive = (n: unknown): n is number => typeof n === "number" && Number.isFinite(n) && n > 0;
const known = (id: unknown): id is PaneId => paneDefinitions.some((p) => p.id === id);
export function valid(value: unknown): value is Saved {
  if (!value || typeof value !== "object") return false;
  const v = value as Saved;
  if (
    !Array.isArray(v.desktop) ||
    !Array.isArray(v.phone) ||
    !v.phone.every(known) ||
    new Set(v.phone).size !== v.phone.length
  )
    return false;
  const seen = new Set<PaneId>();
  return v.desktop.every(
    (r) =>
      r &&
      typeof r === "object" &&
      (r.minHeight === undefined || positive(r.minHeight)) &&
      Array.isArray(r.slots) &&
      r.slots.length >= 1 &&
      r.slots.length <= 4 &&
      r.slots.every((s) => {
        if (!s || typeof s !== "object" || !positive(s.size)) return false;
        if (s.pane === null) return true;
        if (!known(s.pane) || seen.has(s.pane)) return false;
        seen.add(s.pane);
        return true;
      }),
  );
}
export function read(key: string, mode: Mode): Saved {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(key) ?? "null");
    if (valid(value)) return value;
  } catch {
    /* Missing or invalid storage uses the defaults. */
  }
  return defaults(mode);
}
export type VisualSlot = { index: number; slot: Slot };
export type VisualRow = { source: number; slots: VisualSlot[]; minHeight?: number };
export function wrapRows(
  rows: Row[],
  width: number,
  editing: boolean,
  offered: (id: PaneId) => boolean = () => true,
): VisualRow[] {
  const visual: VisualRow[] = [];
  rows.forEach((r, source) => {
    let slots: VisualSlot[] = [];
    let used = 0;
    const flush = () => {
      if (slots.length) visual.push({ source, slots, minHeight: r.minHeight });
      slots = [];
      used = 0;
    };
    r.slots.forEach((slot, index) => {
      if (slot.pane === null ? !editing : !offered(slot.pane)) return;
      const min = minimum(slot);
      if (slots.length && (width < 700 || used + GAP + min > width)) flush();
      used += (slots.length ? GAP : 0) + min;
      slots.push({ index, slot });
    });
    flush();
  });
  return visual;
}
export function slotWidths(slots: Slot[], width: number): number[] {
  const available = Math.max(0, width - GAP * (slots.length - 1));
  const result = slots.map(() => 0);
  let pending = slots.map((_, i) => i);
  let left = available;
  while (pending.length) {
    const scale = Math.max(...pending.map((i) => slots[i]?.size ?? 1));
    const weight = (i: number) => (slots[i]?.size ?? 1) / scale;
    const sum = pending.reduce((s, i) => s + weight(i), 0);
    const small = pending.filter(
      (i) => (left * weight(i)) / sum < Math.min(minimum(slots[i]), available),
    );
    if (!small.length || pending.length === 1) {
      for (const i of pending) result[i] = (left * weight(i)) / sum;
      break;
    }
    for (const i of small) {
      const size = Math.min(minimum(slots[i]), left);
      result[i] = size;
      left -= size;
    }
    pending = pending.filter((i) => !small.includes(i));
  }
  return result;
}
export function resizePair(
  sizes: [number, number],
  pixels: [number, number],
  delta: number,
  mins: [number, number],
  pixelsPerWeight?: number,
): [number, number] {
  const total = pixels[0] + pixels[1];
  const weight = sizes[0] + sizes[1];
  if (!Number.isFinite(delta) || total <= 0 || total < mins[0] + mins[1]) return sizes;
  const left = Math.max(mins[0], Math.min(total - mins[1], pixels[0] + delta));
  if (pixelsPerWeight !== undefined && positive(pixelsPerWeight)) {
    const next: [number, number] = [left / pixelsPerWeight, (total - left) / pixelsPerWeight];
    return next.every(positive) ? next : sizes;
  }
  if (!Number.isFinite(weight)) return sizes;
  return [weight * (left / total), weight * ((total - left) / total)];
}
export function reduceRows(state: Row[], action: Action): Row[] {
  const rows = state.map((r) => ({ ...r, slots: r.slots.map((s) => ({ ...s })) }));
  if (action.type === "add-row") {
    if (
      !Number.isInteger(action.columns) ||
      action.columns < 1 ||
      action.columns > 4 ||
      action.at < 0 ||
      action.at > rows.length
    )
      return state;
    rows.splice(action.at, 0, {
      slots: Array.from({ length: action.columns }, () => ({ pane: null, size: 1 })),
    });
  } else if (action.type === "drop") {
    const from = rows[action.source.row];
    const source = from?.slots[action.source.slot];
    if (!from || !source?.pane) return state;
    const target = action.target;
    if (target.kind === "row") {
      if (target.row < 0 || target.row > rows.length) return state;
      rows.splice(target.row, 0, { slots: [{ pane: source.pane, size: 1 }] });
    } else {
      const to = rows[target.row];
      const slot = to?.slots[target.slot];
      if (!to || !slot || slot === source) return state;
      if (target.kind === "center" && slot.pane !== null) {
        [source.pane, slot.pane] = [slot.pane, source.pane];
        return rows;
      }
      if (target.kind === "center") slot.pane = source.pane;
      else {
        if (to.slots.length >= 4) return state;
        to.slots.splice(target.slot + (target.kind === "right" ? 1 : 0), 0, {
          pane: source.pane,
          size: slot.size,
        });
      }
    }
    from.slots.splice(from.slots.indexOf(source), 1);
  } else {
    const r = rows[action.row];
    if (!r) return state;
    if (action.type === "remove-row") rows.splice(action.row, 1);
    else if (action.type === "height") {
      if (action.value !== undefined && !positive(action.value)) return state;
      r.minHeight = action.value;
    } else if (action.type === "sizes") {
      const left = r.slots[action.left];
      const right = r.slots[action.right];
      if (!left || !right || action.left === action.right || !action.sizes.every(positive))
        return state;
      left.size = action.sizes[0];
      right.size = action.sizes[1];
    } else {
      const s = r.slots[action.slot];
      if (!s) return state;
      if (action.type === "hide") s.pane = null;
      if (action.type === "remove-slot") {
        if (s.pane !== null) return state;
        r.slots.splice(action.slot, 1);
      }
      if (action.type === "beside") {
        if (r.slots.length >= 4) return state;
        r.slots.splice(action.slot + 1, 0, { pane: null, size: s.size });
      }
      if (action.type === "place") {
        if (
          s.pane !== null ||
          !known(action.pane) ||
          rows.some((row) => row.slots.some((slot) => slot.pane === action.pane))
        )
          return state;
        s.pane = action.pane;
      }
    }
  }
  const result = rows.filter((r) => r.slots.length > 0);
  return JSON.stringify(result) === JSON.stringify(state) ? state : result;
}
