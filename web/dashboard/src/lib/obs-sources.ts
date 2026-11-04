import type { ObsSource, ObsSources } from "@/api/types";
import { sceneKey } from "@/lib/hidden-scenes";

/** A source's identity: its item in its scene or group. */
export function sourceKey(source: ObsSource): string {
  return `${source.parent_is_group ? "group" : "scene"}:${source.parent}:${source.item_id}`;
}

/**
 * Whether each source (in the API's order: nested items right after their
 * scene or group) shows on stream: its own eye and every enclosing one's.
 */
export function onStream(sources: ObsSource[]): boolean[] {
  const shownAt: boolean[] = [];
  return sources.map((source) => {
    const shown = source.enabled && (source.depth === 0 || shownAt[source.depth - 1] === true);
    shownAt.length = source.depth;
    shownAt[source.depth] = shown;
    return shown;
  });
}

/** The list with one source shown or hidden, for optimistic updates. */
export function withEnabled(list: ObsSources, key: string, enabled: boolean): ObsSources {
  return {
    ...list,
    sources: list.sources.map((source) =>
      sourceKey(source) === key ? { ...source, enabled } : source,
    ),
  };
}

export type SourceKind =
  | "scene"
  | "group"
  | "media"
  | "camera"
  | "capture"
  | "audio"
  | "browser"
  | "text"
  | "image"
  | "other";

const KINDS: [SourceKind, (kind: string) => boolean][] = [
  ["media", (kind) => ["ffmpeg_source", "vlc_source", "irl_source"].includes(kind)],
  [
    "camera",
    (kind) =>
      ["dshow_input", "v4l2_input", "macos-avcapture"].includes(kind) ||
      kind.startsWith("av_capture"),
  ],
  [
    "capture",
    (kind) =>
      /^(monitor|window|game|screen|display)_capture/.test(kind) ||
      kind.startsWith("xshm_input") ||
      kind.startsWith("pipewire"),
  ],
  [
    "audio",
    (kind) =>
      kind.startsWith("wasapi_") ||
      kind.startsWith("pulse_") ||
      kind.startsWith("coreaudio_") ||
      kind.startsWith("alsa_") ||
      kind.startsWith("jack_") ||
      kind === "sck_audio_capture",
  ],
  ["browser", (kind) => kind === "browser_source"],
  ["text", (kind) => kind.startsWith("text_")],
  [
    "image",
    (kind) => kind === "image_source" || kind === "slideshow" || kind.startsWith("slideshow_"),
  ],
];

/** What a source is, for its icon. */
export function sourceKind(source: ObsSource): SourceKind {
  if (source.is_group) return "group";
  if (source.is_scene) return "scene";
  return inputKind(source.input_kind ?? "");
}

/** What an input of OBS kind `kind` is, for its icon. */
export function inputKind(kind: string): SourceKind {
  return KINDS.find(([, test]) => test(kind))?.[0] ?? "other";
}

export interface SourceSection {
  /** `null` for the scene on air itself, else the nested scene or group. */
  parent: string | null;
  parentIsGroup: boolean;
  /** Whether that scene or group is on stream (always for the scene on air). */
  parentShown: boolean;
  items: { source: ObsSource; shown: boolean }[];
}

/**
 * The list split by where each source sits: the scene on air first, then each
 * nested scene or group in the order they appear.
 */
export function sourceSections(sources: ObsSource[]): SourceSection[] {
  const shown = onStream(sources);
  const sections = new Map<string, SourceSection>();
  // The first time a scene or group appears is the one the API expanded.
  const containerShown = new Map<string, boolean>();
  sources.forEach((source, index) => {
    if (source.is_scene || source.is_group) {
      const key = `${source.is_group}:${source.name}`;
      if (!containerShown.has(key)) containerShown.set(key, shown[index] ?? false);
    }
    const root = source.depth === 0;
    const key = root ? "root" : `${source.parent_is_group}:${source.parent}`;
    let section = sections.get(key);
    if (!section) {
      section = {
        parent: root ? null : source.parent,
        parentIsGroup: !root && source.parent_is_group,
        parentShown: root || (containerShown.get(key) ?? true),
        items: [],
      };
      sections.set(key, section);
    }
    section.items.push({ source, shown: shown[index] ?? false });
  });
  return [...sections.values()];
}

/** The sections without the sources removed from the list (`sceneKey`s); empty sections go too. */
export function keptSections(sections: SourceSection[], removed: Set<string>): SourceSection[] {
  return sections
    .map((section) => ({
      ...section,
      items: section.items.filter(({ source }) => !removed.has(sceneKey(source))),
    }))
    .filter((section) => section.items.length > 0);
}

export type EditableKind = "media" | "vlc" | "browser" | "text";

/** Whether a source can be edited from the dashboard, and as what. */
export function editableKind(source: ObsSource): EditableKind | null {
  switch (source.input_kind) {
    case "ffmpeg_source":
      return "media";
    case "vlc_source":
      return "vlc";
    case "browser_source":
      return "browser";
    default:
      // Text (GDI+) on Windows, Text (FreeType 2) elsewhere, in any version.
      return /^(text_gdiplus|text_ft2_source)(_v\d+)?$/.test(source.input_kind ?? "")
        ? "text"
        : null;
  }
}

/** Where a source's block (it, and the items of a nested scene or group) ends in the list. */
function blockEnd(sources: ObsSource[], start: number): number {
  const depth = sources[start]?.depth ?? 0;
  let end = start + 1;
  while (end < sources.length && (sources[end]?.depth ?? 0) > depth) end++;
  return end;
}

/**
 * The list with one source moved to `position` among the items of its scene
 * or group (counted from the top; what's in a nested scene or group moves
 * with it), for optimistic updates. Unchanged when it's there already.
 */
export function withMovedTo(list: ObsSources, key: string, position: number): ObsSources {
  const sources = list.sources;
  const moving = sources.find((source) => sourceKey(source) === key);
  if (!moving) return list;
  // Its siblings' blocks (each with what's in it), which sit one after another.
  const blocks: [number, number][] = [];
  sources.forEach((source, index) => {
    if (
      source.depth === moving.depth &&
      source.parent === moving.parent &&
      source.parent_is_group === moving.parent_is_group
    ) {
      blocks.push([index, blockEnd(sources, index)]);
    }
  });
  const from = blocks.findIndex(([start]) => sources[start] === moving);
  const to = Math.min(Math.max(position, 0), blocks.length - 1);
  if (from === to) return list;
  const reordered = [...blocks];
  const [block] = reordered.splice(from, 1);
  if (block) reordered.splice(to, 0, block);
  const first = blocks[0]?.[0] ?? 0;
  const last = blocks.at(-1)?.[1] ?? first;
  return {
    ...list,
    sources: [
      ...sources.slice(0, first),
      ...reordered.flatMap(([start, end]) => sources.slice(start, end)),
      ...sources.slice(last),
    ],
  };
}

/** The list without one source (and what's in it, for a nested scene), for optimistic updates. */
export function withRemoved(list: ObsSources, key: string): ObsSources {
  const index = list.sources.findIndex((source) => sourceKey(source) === key);
  if (index < 0) return list;
  const end = blockEnd(list.sources, index);
  return { ...list, sources: [...list.sources.slice(0, index), ...list.sources.slice(end)] };
}

/** Whether a source can be removed from the dashboard: not a group, nor in one. */
export function removable(source: ObsSource): boolean {
  return !source.is_group && !source.parent_is_group;
}

/** Whether a source can be duplicated from the dashboard: an input, not a scene or group. */
export function duplicable(source: ObsSource): boolean {
  return !source.is_scene && !source.is_group;
}

/**
 * A name for a copy of `name`, like OBS's: "Cam" → "Cam 2", "Cam 2" → "Cam 3",
 * skipping names already `taken`.
 */
export function copyName(name: string, taken: Iterable<string>): string {
  const used = new Set(taken);
  const match = /^(.*?) (\d+)$/.exec(name);
  const base = match?.[1] ?? name;
  let number = match ? Number(match[2]) + 1 : 2;
  while (used.has(`${base} ${number}`)) number++;
  return `${base} ${number}`;
}
