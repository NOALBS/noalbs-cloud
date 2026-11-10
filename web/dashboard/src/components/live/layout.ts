import { t } from "@lingui/core/macro";

export const TILE_IDS = [
  "hero",
  "actions",
  "servers",
  "scenes",
  "sources",
  "devices",
  "belabox",
  "moblin",
  "audio",
  "activity",
  "obs",
  "markers",
] as const;
export type TileId = (typeof TILE_IDS)[number];
export type Columns = 12 | 16;
export type Preset = "default" | "operator" | "compact";
export interface Tile {
  id: TileId;
  span: number;
}
export interface Availability {
  belabox: boolean;
  moblin: boolean;
}
export interface DesktopLayout {
  v: 1;
  preset: Preset;
  grids: Partial<Record<Columns, Tile[]>>;
}
export const ALL_DEVICES: Availability = { belabox: true, moblin: true };
export const tileNames = (): Record<TileId, string> => ({
  hero: t`Stream`,
  actions: t`Actions`,
  servers: t`Stream servers`,
  scenes: t`Scenes`,
  sources: t`Sources`,
  devices: t`Devices`,
  belabox: t`BELABOX`,
  moblin: t`Moblin`,
  audio: t`Audio`,
  activity: t`Activity`,
  obs: t`OBS`,
  markers: t`Markers`,
});
export const minSpan = (id: TileId): number => (id === "hero" ? 6 : id === "sources" ? 4 : 3);
export const columnCount = (width: number): Columns => (width >= 1920 ? 16 : 12);
export function clampSpan(id: TileId, span: unknown, cols: Columns): number {
  const min = Math.min(minSpan(id), cols);
  return Math.max(
    min,
    Math.min(cols, typeof span === "number" && Number.isFinite(span) ? Math.round(span) : min),
  );
}
const tiles = (...entries: [TileId, number][]): Tile[] =>
  entries.map(([id, span]) => ({ id, span }));
export const PRESETS: Record<Columns, Record<Preset, readonly Tile[]>> = {
  12: {
    default: tiles(
      ["hero", 9],
      ["actions", 3],
      ["servers", 3],
      ["scenes", 5],
      ["activity", 4],
      ["sources", 5],
      ["devices", 4],
      ["audio", 3],
    ),
    operator: tiles(
      ["hero", 8],
      ["actions", 4],
      ["scenes", 5],
      ["sources", 7],
      ["servers", 3],
      ["devices", 3],
      ["audio", 3],
      ["activity", 3],
    ),
    compact: tiles(["hero", 9], ["actions", 3], ["scenes", 8], ["servers", 4]),
  },
  16: {
    default: tiles(
      ["hero", 9],
      ["actions", 3],
      ["obs", 4],
      ["servers", 3],
      ["scenes", 5],
      ["belabox", 4],
      ["moblin", 4],
      ["sources", 6],
      ["audio", 4],
      ["markers", 3],
      ["activity", 3],
    ),
    operator: tiles(
      ["hero", 8],
      ["actions", 4],
      ["obs", 4],
      ["scenes", 6],
      ["sources", 6],
      ["audio", 4],
      ["servers", 4],
      ["belabox", 4],
      ["moblin", 4],
      ["activity", 4],
    ),
    compact: tiles(["hero", 9], ["actions", 3], ["servers", 4], ["scenes", 10], ["activity", 6]),
  },
};
function object(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
const available = (id: TileId, availability: Availability) =>
  (id !== "belabox" && id !== "moblin") || availability[id];
export function normalize(input: unknown, cols: Columns, availability: Availability): Tile[] {
  const seen = new Set<string>();
  if (!Array.isArray(input)) return [];
  return input.flatMap((entry: unknown) => {
    if (!object(entry) || !TILE_IDS.includes(entry.id as TileId) || seen.has(entry.id as string))
      return [];
    const id = entry.id as TileId;
    seen.add(id);
    return available(id, availability) ? [{ id, span: clampSpan(id, entry.span, cols) }] : [];
  });
}
function parse(raw: string | null): Record<string, unknown> | null {
  try {
    const value: unknown = JSON.parse(raw ?? "null");
    return object(value) && value.v === 1 ? value : null;
  } catch {
    return null;
  }
}
export function parseLayout(raw: string | null, availability: Availability): DesktopLayout | null {
  const value = parse(raw);
  if (!value) return null;
  const preset =
    value.preset === "operator" || value.preset === "compact" ? value.preset : "default";
  const grids: DesktopLayout["grids"] = {};
  if (object(value.grids))
    for (const cols of [12, 16] as const) {
      if (Array.isArray(value.grids[cols]))
        grids[cols] = normalize(value.grids[cols], cols, availability);
    }
  return { v: 1, preset, grids };
}
export const serializeLayout = (layout: DesktopLayout): string => JSON.stringify(layout);
export const layoutStorageKey = (userId: string, switcherId: string): string =>
  `noalbs-live-layout:${userId}:${switcherId}`;
export function effectiveTiles(
  layout: DesktopLayout | null,
  cols: Columns,
  availability: Availability,
): Tile[] {
  return normalize(
    layout?.grids[cols] ?? PRESETS[cols][layout?.preset ?? "default"],
    cols,
    availability,
  );
}
export function moveTile(items: readonly Tile[], id: TileId, index: number): Tile[] {
  const item = items.find((tile) => tile.id === id);
  if (!item) return [...items];
  const next = items.filter((tile) => tile.id !== id);
  next.splice(Math.max(0, Math.min(next.length, Math.round(index))), 0, item);
  return next;
}
export const resizeTile = (
  items: readonly Tile[],
  id: TileId,
  span: number,
  cols: Columns,
): Tile[] => items.map((tile) => (tile.id === id ? { id, span: clampSpan(id, span, cols) } : tile));
export const hideTile = (items: readonly Tile[], id: TileId): Tile[] =>
  items.filter((tile) => tile.id !== id);
export function addTile(
  items: readonly Tile[],
  id: TileId,
  cols: Columns,
  availability: Availability,
): Tile[] {
  return normalize([...items, { id, span: minSpan(id) }], cols, availability);
}
export const hiddenTiles = (items: readonly Tile[], availability: Availability): TileId[] =>
  TILE_IDS.filter((id) => available(id, availability) && !items.some((tile) => tile.id === id));

export const PHONE_SECTION_IDS = ["servers", "scenes", "devices", "activity"] as const;
export type PhoneSectionId = (typeof PHONE_SECTION_IDS)[number];
export interface PhoneSection {
  id: PhoneSectionId;
  hidden: boolean;
}
export interface PhoneLayout {
  v: 1;
  sections: PhoneSection[];
}
export function normalizePhone(input: unknown): PhoneSection[] {
  const sections: PhoneSection[] = [];
  if (Array.isArray(input))
    for (const entry of input) {
      if (
        object(entry) &&
        PHONE_SECTION_IDS.includes(entry.id as PhoneSectionId) &&
        !sections.some((section) => section.id === entry.id)
      ) {
        sections.push({ id: entry.id as PhoneSectionId, hidden: entry.hidden === true });
      }
    }
  for (const id of PHONE_SECTION_IDS)
    if (!sections.some((section) => section.id === id)) sections.push({ id, hidden: false });
  return sections;
}
export function parsePhoneLayout(raw: string | null): PhoneLayout | null {
  const value = parse(raw);
  return value ? { v: 1, sections: normalizePhone(value.sections) } : null;
}
export const serializePhoneLayout = (layout: PhoneLayout): string => JSON.stringify(layout);
export const phoneStorageKey = (userId: string, switcherId: string): string =>
  `noalbs-live-phone:${userId}:${switcherId}`;
