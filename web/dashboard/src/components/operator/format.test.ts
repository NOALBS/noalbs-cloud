import { describe, expect, it } from "vitest";
import { problemLine, stateChip } from "./format";
import type { OperatorItem } from "./model";

const base: OperatorItem = {
  switcher_id: "one",
  switcher_name: "Main",
  workspace_id: "w",
  workspace_name: "Studio",
  workspace_slug: "studio",
  can_control: true,
  connected: true,
  obs_connected: true,
  streaming: true,
  recording: false,
  paused: false,
  state: "offline",
};
const now = Date.parse("2026-01-01T12:00:00Z");
describe("operator state text", () => {
  it("ticks attention and stop durations, clamping expired countdowns", () => {
    const row = {
      ...base,
      attention: "no_feed" as const,
      state_since: "2026-01-01T11:58:18Z",
      stops_at: "2026-01-01T12:13:18Z",
    };
    expect(problemLine(row, now)).toBe("No feed for 01:42 · the stream stops in 13:18");
    expect(problemLine(row, now + 1000)).toBe("No feed for 01:43 · the stream stops in 13:17");
    expect(problemLine({ ...row, stops_at: "2026-01-01T11:59:00Z" }, now)).toContain("00:00");
    expect(problemLine({ ...base, attention: "low" }, now)).toBe("Low bitrate");
    expect(problemLine(base, now)).toBeNull();
  });
  it("keeps the lost stream duration frozen while time since loss grows", () => {
    const row = {
      ...base,
      attention: "obs_lost" as const,
      obs_lost: { since: "2026-01-01T11:57:00Z", stream_started_at: "2026-01-01T10:51:40Z" },
    };
    expect(problemLine(row, now)).toBe("OBS disconnected 3 min ago · was live for 1:05:20");
    expect(problemLine(row, now + 60000)).toBe("OBS disconnected 4 min ago · was live for 1:05:20");
  });
  it("uses green only for a healthy streaming feed", () => {
    expect(stateChip({ ...base, state: "live" }).className).toContain("text-live");
    expect(stateChip({ ...base, state: "live", streaming: false }).label).toBe("Not streaming");
    expect(stateChip({ ...base, paused: true }).label).toBe("Paused");
    expect(stateChip({ ...base, obs_connected: false }).label).toBe("No OBS");
    expect(stateChip(base).label).toBe("BRB");
  });
});
