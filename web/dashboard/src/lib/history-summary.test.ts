import { describe, expect, it } from "vitest";
import type { StreamSummary } from "@/api/types";
import { groupByDay } from "@/lib/history";
import { historySummary } from "./history-summary";

const now = Date.parse("2026-06-15T12:00:00Z");
const stream = (started_at: string, live_seconds = 100): StreamSummary => ({
  id: started_at,
  started_at,
  ended_at: started_at,
  duration_seconds: 120,
  live_seconds,
  low_seconds: 15,
  offline_seconds: 5,
  auto_switches: 2,
});
describe("historySummary", () => {
  it("adds recorded seconds rather than averaging stream percentages", () => {
    expect(
      historySummary([stream("2026-06-14T12:00:00Z"), stream("2026-06-13T12:00:00Z", 200)], now),
    ).toEqual({
      count: 2,
      duration: 240,
      live_seconds: 300,
      low_seconds: 30,
      offline_seconds: 10,
      switches: 4,
    });
  });
  it("handles no streams", () => expect(historySummary([], now).count).toBe(0));
  it("excludes old, future and invalid dates but includes the cutoff", () => {
    expect(
      historySummary(
        [
          stream("2026-05-16T12:00:00Z"),
          stream("2026-05-16T11:59:59Z"),
          stream("2026-06-16T12:00:00Z"),
          stream("invalid"),
        ],
        now,
      ).count,
    ).toBe(1);
  });
});
describe("relative history days", () => {
  it("labels today, yesterday and older dates in local time", () => {
    const today = new Date(2026, 5, 15, 12);
    const groups = groupByDay(
      [
        stream(today.toISOString()),
        stream(new Date(2026, 5, 14, 12).toISOString()),
        stream(new Date(2026, 5, 12, 12).toISOString()),
      ],
      "en",
      today,
    );
    expect(groups[0]?.label).toBe("Today");
    expect(groups[1]?.label).toBe("Yesterday");
    expect(groups[2]?.label).toContain("June");
  });
});
