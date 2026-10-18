import { useCallback, useState } from "react";

/**
 * The Live page's OBS preview: stills of the scene on air, fetched at the rate
 * the viewer picks. Every frame is sent by the streamer's OBS computer, so rate,
 * size and quality trade smoothness for its upload (and the viewer's download).
 * Kept in this browser only.
 */
export interface PreviewSettings {
  /** `null`: the default for the layout (on for desktop, off on phones). */
  enabled: boolean | null;
  /** Frames per second. */
  fps: number;
  /** Width in pixels; the height follows OBS's aspect ratio. */
  width: number;
  /** JPEG quality, 1 to 100. */
  quality: number;
}

export const FPS_OPTIONS = [0.5, 1, 2, 5] as const;
export const WIDTH_OPTIONS = [320, 480, 640, 960, 1280] as const;
export const QUALITY_OPTIONS = [
  { value: 30, label: "Low" },
  { value: 60, label: "Medium" },
  { value: 80, label: "High" },
  { value: 95, label: "Best" },
] as const;

export const DEFAULT_PREVIEW: PreviewSettings = { enabled: null, fps: 1, width: 640, quality: 60 };

const KEY = "noalbs-obs-preview";

const oneOf = <T extends number>(options: readonly T[], value: unknown, fallback: T): T =>
  options.includes(value as T) ? (value as T) : fallback;

/** Reads stored settings, falling back to the defaults for anything missing or odd. */
export function parsePreview(raw: string | null): PreviewSettings {
  let value: Record<string, unknown> = {};
  try {
    const parsed: unknown = raw ? JSON.parse(raw) : {};
    if (typeof parsed === "object" && parsed !== null) value = parsed as Record<string, unknown>;
  } catch {
    // Malformed: defaults.
  }
  return {
    enabled: typeof value.enabled === "boolean" ? value.enabled : null,
    fps: oneOf(FPS_OPTIONS, value.fps, DEFAULT_PREVIEW.fps as 1),
    width: oneOf(WIDTH_OPTIONS, value.width, DEFAULT_PREVIEW.width as 640),
    quality: oneOf(
      QUALITY_OPTIONS.map((option) => option.value),
      value.quality,
      DEFAULT_PREVIEW.quality as 60,
    ),
  };
}

/** Whether the preview runs: the viewer's choice, else on for desktop only. */
export function previewOn(settings: PreviewSettings, desktop: boolean): boolean {
  return settings.enabled ?? desktop;
}

export function fpsLabel(fps: number): string {
  return fps < 1 ? `1 every ${Math.round(1 / fps)} s` : `${fps} fps`;
}

/** Roughly what the preview costs at this frame size, in kbps. */
export function bandwidthKbps(frameBytes: number, fps: number): number {
  return Math.round((frameBytes * 8 * fps) / 1000);
}

export function formatBytes(bytes: number): string {
  return bytes < 1024 ? `${bytes} B` : `${Math.round(bytes / 1024)} KB`;
}

function read(): PreviewSettings {
  try {
    return parsePreview(localStorage.getItem(KEY));
  } catch {
    return DEFAULT_PREVIEW;
  }
}

export function usePreviewSettings() {
  const [settings, setSettings] = useState(read);
  const update = useCallback((patch: Partial<PreviewSettings>) => {
    setSettings((current) => {
      const next = { ...current, ...patch };
      try {
        localStorage.setItem(KEY, JSON.stringify(next));
      } catch {
        // Storage can be unavailable (private mode); the choice then lasts this visit.
      }
      return next;
    });
  }, []);
  return [settings, update] as const;
}
