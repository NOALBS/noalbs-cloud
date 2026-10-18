import { describe, expect, it } from "vitest";
import type { SwitcherEvent } from "@/api/types";
import { describeEvent, formatUptime } from "./live-format";

const at = "2026-09-24T12:00:00Z";

describe("formatUptime", () => {
  it("shows hours only when needed", () => {
    expect(formatUptime(at, Date.parse("2026-09-24T12:14:37Z"))).toBe("14:37");
    expect(formatUptime(at, Date.parse("2026-09-24T14:14:37Z"))).toBe("2:14:37");
  });
});

describe("describeEvent", () => {
  const base = { id: "1", created_at: at };

  it("explains automatic switches with their cause", () => {
    const event: SwitcherEvent = {
      ...base,
      kind: "scene_switched",
      data: { from: "LIVE", to: "LOW", reason: "auto", server: "BELABOX Cloud", bitrate: 640 },
    };
    expect(describeEvent(event)).toEqual({
      title: "LIVE → LOW",
      detail: "Automatic · BELABOX Cloud at 640 kbps",
    });
  });

  it("says who did it, or where", () => {
    expect(
      describeEvent({ ...base, kind: "stream_started", data: { by: "mod_kate" } }).detail,
    ).toBe("by mod_kate");
    expect(describeEvent({ ...base, kind: "stream_stopped", data: {} }).detail).toBe("in OBS");
    expect(describeEvent({ ...base, kind: "offline_timeout", data: { minutes: 5 } }).detail).toBe(
      "No feed for 5 min on BRB, so the stream was stopped",
    );
    expect(describeEvent({ ...base, kind: "ending_timeout", data: { minutes: 3 } }).detail).toBe(
      "Ending scene on for 3 min, so the stream was stopped",
    );
    expect(
      describeEvent({
        ...base,
        kind: "scene_switched",
        data: { to: "Privacy", reason: "obs" },
      }),
    ).toEqual({ title: "Switched to Privacy", detail: "Changed in OBS" });
  });

  it("says which source was shown or hidden, and where", () => {
    expect(
      describeEvent({
        ...base,
        kind: "source_toggled",
        data: { source: "Camera", scene: "LIVE", enabled: false, by: "kate" },
      }),
    ).toEqual({ title: "Hid Camera", detail: "In LIVE by kate" });
  });

  it("says which input was muted", () => {
    expect(
      describeEvent({
        ...base,
        kind: "input_muted",
        data: { input: "Mic/Aux", muted: true, by: "kate" },
      }),
    ).toEqual({ title: "Muted Mic/Aux", detail: "by kate" });
  });

  it("describes BELABOX changes", () => {
    expect(
      describeEvent({
        ...base,
        kind: "belabox_stream_started",
        data: { by: "the OBS stream" },
      }),
    ).toEqual({ title: "BELABOX started", detail: "by the OBS stream" });
    expect(
      describeEvent({
        ...base,
        kind: "belabox_interface_toggled",
        data: { name: "usb0", enabled: false },
      }),
    ).toEqual({ title: "BELABOX usb0 off", detail: "on the BELABOX" });
    expect(
      describeEvent({
        ...base,
        kind: "belabox_configured",
        data: { what: "stream_settings", by: "kate" },
      }),
    ).toEqual({
      title: "BELABOX stream settings changed",
      detail: "by kate, used from its next start",
    });
    expect(
      describeEvent({ ...base, kind: "belabox_configured", data: { what: "wifi", by: "kate" } }),
    ).toEqual({ title: "BELABOX WiFi changed", detail: "by kate" });
  });

  it("says what Moblin did and who asked", () => {
    expect(
      describeEvent({ ...base, kind: "moblin_stream_started", data: { by: "the OBS stream" } }),
    ).toEqual({ title: "Moblin went live", detail: "by the OBS stream" });
    expect(describeEvent({ ...base, kind: "moblin_mute_changed", data: { muted: true } })).toEqual({
      title: "Moblin muted",
      detail: "on the phone",
    });
    expect(
      describeEvent({
        ...base,
        kind: "moblin_bitrate_changed",
        data: { kbps: 6000, by: "kate" },
      }),
    ).toEqual({ title: "Moblin bitrate 6,000 kbps", detail: "by kate" });
    expect(
      describeEvent({
        ...base,
        kind: "moblin_connection_changed",
        data: { name: "Cellular", enabled: false, priority: 1, by: "kate" },
      }),
    ).toEqual({ title: "Moblin Cellular off", detail: "by kate" });
  });

  it("names the sources a fix reconnected", () => {
    expect(
      describeEvent({
        ...base,
        kind: "feed_fixed",
        data: { sources: ["BELABOX SRT", "Backup"], by: "mod_kate (Twitch chat)" },
      }),
    ).toEqual({
      title: "Fixed the feed",
      detail: "BELABOX SRT, Backup by mod_kate (Twitch chat)",
    });
  });

  it("describes a marker", () => {
    const marker = { marker_id: "m", offset_seconds: 3725 };
    expect(
      describeEvent({
        ...base,
        kind: "marker_created",
        data: { ...marker, description: "Clutch play", by: "mod_kate (Twitch chat)" },
      }),
    ).toEqual({
      title: "Marker: Clutch play",
      detail: "At 1:02:05 by mod_kate (Twitch chat)",
    });
    expect(
      describeEvent({
        ...base,
        kind: "marker_created",
        data: { ...marker, description: null, by: null },
      }),
    ).toEqual({ title: "Marker", detail: "At 1:02:05 from the dashboard" });
  });
});
