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
  const kind = source.input_kind ?? "";
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
