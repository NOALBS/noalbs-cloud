import { describe, expect, it } from "vitest";
import { canUseRowActions, type OperatorItem } from "./model";
import { phoneDetail, phoneReading } from "./phone-format";

const now = Date.parse("2026-11-10T12:00:00Z");
const item: OperatorItem = {
  switcher_id: "s",
  switcher_name: "Main",
  workspace_id: "w",
  workspace_name: "Studio",
  workspace_slug: "studio",
  connected: true,
  can_control: true,
  obs_connected: true,
  streaming: true,
  recording: false,
  paused: false,
  state: "live",
  scene: "IRL",
  stream_started_at: new Date(now - 3600000).toISOString(),
  server: { id: "server", name: "Feed", kind: "belabox", bitrate: 6200, rtt: 20 },
};
describe("phone readings and control eligibility", () => {
  it("shows live bitrate, attention time, No OBS and paused without invented metrics", () => {
    expect(phoneReading(item, now)).toBe("6,200");
    expect(
      phoneReading(
        { ...item, attention: "no_feed", attention_since: new Date(now - 102000).toISOString() },
        now,
      ),
    ).toBe("1:42");
    expect(phoneReading({ ...item, obs_connected: false }, now)).toBe("No OBS");
    expect(phoneReading({ ...item, paused: true }, now)).toBe("Paused");
    expect(phoneReading({ ...item, streaming: false }, now)).toBe("—");
    expect(phoneReading({ ...item, server: null }, now)).toBe("—");
    expect(
      phoneReading(
        { ...item, attention: "no_feed", attention_since: null, state_since: null },
        now,
      ),
    ).toBe("—");
  });
  it("formats scene/uptime, idle, empty and attention second lines", () => {
    expect(phoneDetail(item, now)).toBe("On IRL · 1:00:00");
    expect(phoneDetail({ ...item, streaming: false }, now)).toBe("In OBS: IRL");
    expect(phoneDetail({ ...item, scene: null }, now)).toBe("No scene");
    expect(
      phoneDetail(
        { ...item, attention: "low", attention_since: new Date(now - 48000).toISOString() },
        now,
      ),
    ).toBe("Low bitrate for 00:48");
  });
  it("allows only authorized, connected, unpaused rows with OBS", () => {
    expect(canUseRowActions(item)).toBe(true);
    for (const patch of [
      { can_control: false },
      { connected: false },
      { obs_connected: false },
      { paused: true },
    ])
      expect(canUseRowActions({ ...item, ...patch })).toBe(false);
    expect(canUseRowActions({ ...item, streaming: false })).toBe(true);
  });
});
