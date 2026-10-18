import { describe, expect, it } from "vitest";
import {
  bandwidthKbps,
  DEFAULT_PREVIEW,
  formatBytes,
  fpsLabel,
  parsePreview,
  previewOn,
} from "./obs-preview";

describe("OBS preview settings", () => {
  it("fall back to the defaults for anything missing or unknown", () => {
    expect(parsePreview(null)).toEqual(DEFAULT_PREVIEW);
    expect(parsePreview("not json")).toEqual(DEFAULT_PREVIEW);
    expect(parsePreview('{"enabled":true,"fps":2,"width":999,"quality":80}')).toEqual({
      enabled: true,
      fps: 2,
      width: 640,
      quality: 80,
    });
  });

  it("are off on phones and on for desktop until the viewer chooses", () => {
    expect(previewOn(DEFAULT_PREVIEW, true)).toBe(true);
    expect(previewOn(DEFAULT_PREVIEW, false)).toBe(false);
    expect(previewOn({ ...DEFAULT_PREVIEW, enabled: true }, false)).toBe(true);
    expect(previewOn({ ...DEFAULT_PREVIEW, enabled: false }, true)).toBe(false);
  });

  it("describe rates and what they cost", () => {
    expect(fpsLabel(0.5)).toBe("1 every 2 s");
    expect(fpsLabel(2)).toBe("2 fps");
    // 40 KB frames at 2 fps ≈ 655 kbps.
    expect(bandwidthKbps(40_960, 2)).toBe(655);
    expect(formatBytes(900)).toBe("900 B");
    expect(formatBytes(40_960)).toBe("40 KB");
  });
});
