import type { components } from "@/api/schema";
import type { EditableKind } from "@/lib/obs-sources";

export type SourceSettings = components["schemas"]["SourceSettings"];
export type MediaSettings = Extract<SourceSettings, { kind: "media" }>;
export type VlcSettings = Extract<SourceSettings, { kind: "vlc" }>;
export type BrowserSettings = Extract<SourceSettings, { kind: "browser" }>;
export type TextSettings = Extract<SourceSettings, { kind: "text" }>;

/** OBS's own CSS for a new browser source: a see-through page. */
export const DEFAULT_BROWSER_CSS =
  "body { background-color: rgba(0, 0, 0, 0); margin: 0px auto; overflow: hidden; }";

/** A new source's settings: OBS's defaults, but browser sources sized for 1080p overlays. */
export function newSettings<K extends EditableKind>(kind: K): Extract<SourceSettings, { kind: K }> {
  return defaults(kind) as Extract<SourceSettings, { kind: K }>;
}

function defaults(kind: EditableKind): SourceSettings {
  switch (kind) {
    case "media":
      return {
        kind,
        url: "",
        input_format: "",
        buffering_mb: 2,
        reconnect_delay_sec: 10,
        hw_decode: false,
        restart_on_activate: true,
        close_when_inactive: false,
        clear_on_media_end: true,
      };
    case "vlc":
      return {
        kind,
        playlist: [""],
        loop_playlist: true,
        shuffle: false,
        playback: "stop_restart",
        network_caching_ms: 400,
      };
    case "browser":
      return {
        kind,
        url: "",
        width: 1920,
        height: 1080,
        fps: null,
        css: DEFAULT_BROWSER_CSS,
        shutdown: false,
        restart_when_active: false,
        reroute_audio: false,
      };
    case "text":
      // An empty font face keeps OBS's own for the computer it runs on.
      return {
        kind,
        text: "",
        font_face: "",
        font_size: 96,
        bold: false,
        italic: false,
        color: "#ffffff",
        outline: false,
      };
  }
}

/** Whether what it plays is a file on the OBS computer (not shown, kept unless replaced). */
export function playsLocalFile(settings: SourceSettings): boolean {
  switch (settings.kind) {
    case "vlc":
      return settings.playlist == null;
    case "text":
      return settings.text == null;
    default:
      return settings.url == null;
  }
}

/**
 * The settings to send. What it plays goes only when it was changed (`replaced`),
 * so a URL the dashboard didn't show is kept; URLs are trimmed and empty rows dropped.
 */
export function settingsToSave(settings: SourceSettings, replaced: boolean): SourceSettings {
  switch (settings.kind) {
    case "vlc":
      return {
        ...settings,
        playlist: replaced
          ? (settings.playlist ?? []).map((url) => url.trim()).filter((url) => url !== "")
          : null,
      };
    case "text":
      // Text is sent as typed: spaces and line breaks are part of it.
      return { ...settings, text: replaced ? (settings.text ?? "") : null };
    default:
      return { ...settings, url: replaced ? (settings.url ?? "").trim() : null };
  }
}

/** What a source shows (its URL, playlist or text), to tell whether it was changed. */
export function whatItShows(settings: SourceSettings): unknown {
  switch (settings.kind) {
    case "vlc":
      return settings.playlist;
    case "text":
      return settings.text;
    default:
      return settings.url;
  }
}
