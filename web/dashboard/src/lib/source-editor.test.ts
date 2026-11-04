import { describe, expect, it } from "vitest";
import { newSettings, playsLocalFile, settingsToSave } from "./source-editor";

describe("settingsToSave", () => {
  it("sends what it plays only when it was replaced", () => {
    const media = { ...newSettings("media"), url: " srt://ingest:4000 " };
    expect(settingsToSave(media, true)).toMatchObject({ url: "srt://ingest:4000" });
    expect(settingsToSave(media, false)).toMatchObject({ url: null, buffering_mb: 2 });
  });

  it("drops a VLC playlist's empty rows", () => {
    const vlc = { ...newSettings("vlc"), playlist: ["srt://a:1", " ", " rtmp://b/live "] };
    expect(settingsToSave(vlc, true)).toMatchObject({ playlist: ["srt://a:1", "rtmp://b/live"] });
    expect(settingsToSave(vlc, false)).toMatchObject({ playlist: null });
  });
});

describe("playsLocalFile", () => {
  it("is what the API couldn't show", () => {
    expect(playsLocalFile(newSettings("browser"))).toBe(false);
    expect(playsLocalFile({ ...newSettings("browser"), url: null })).toBe(true);
    expect(playsLocalFile({ ...newSettings("vlc"), playlist: null })).toBe(true);
  });
});

describe("text sources", () => {
  it("send their text as typed, or keep the file they read", () => {
    const text = { ...newSettings("text"), text: "  BRB\n5 min " };
    expect(settingsToSave(text, true)).toMatchObject({ text: "  BRB\n5 min " });
    expect(settingsToSave(text, false)).toMatchObject({ text: null, font_size: 96 });
    expect(playsLocalFile({ ...text, text: null })).toBe(true);
  });
});
