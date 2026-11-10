import { describe, expect, it } from "vitest";
import type { BelaboxStatus } from "@/api/types";
import { belaboxView } from "@/lib/belabox";
import { moblinView } from "@/lib/moblin";
import { belaboxSummary, connectSummary, moblinSummary } from "./device-summary";

const now = Date.parse("2026-11-10T12:00:00Z");
const belabox: BelaboxStatus = {
  updated_at: "2026-11-10T11:59:58Z",
  link: "online",
  streaming: true,
  total_kbps: 4500,
  max_br: 6000,
  interfaces: [],
  sensors: [],
  notifications: [],
};
describe("device summaries", () => {
  it("uses live encoder status rather than just saved configuration", () => {
    expect(belaboxSummary(belaboxView(belabox, now), true)).toBe("Connected");
    expect(belaboxSummary(belaboxView({ ...belabox, link: "encoder_offline" }, now), true)).toBe(
      "Encoder offline",
    );
    expect(belaboxSummary(belaboxView({ ...belabox, link: "invalid_key" }, now), true)).toBe(
      "Remote key rejected",
    );
    expect(belaboxSummary(belaboxView({ ...belabox, link: "connecting" }, now), true)).toBe(
      "Connecting…",
    );
  });
  it("does not report missing or stale data as connected", () => {
    expect(belaboxSummary(belaboxView(undefined, now), true)).toBe("Loading status…");
    expect(belaboxSummary(belaboxView(null, now), true)).toBe("Not connected");
    expect(belaboxSummary(belaboxView(belabox, now + 46000), true)).toBe("Not connected");
    expect(belaboxSummary(belaboxView(null, now), false)).toBe("Not set up");
    expect(moblinSummary(moblinView(undefined, now), true)).toBe("Loading status…");
    expect(moblinSummary(moblinView(null, now), false)).toBe("Not set up");
    expect(moblinSummary(moblinView(null, now), true)).toBe("Not connected");
  });
  it("distinguishes paired computers from their live connection", () => {
    expect(connectSummary(2, true)).toBe("2 paired computers · online");
    expect(connectSummary(1, false)).toBe("1 paired computer · offline");
    expect(connectSummary(2, undefined)).toBe("2 paired computers · checking status");
    expect(connectSummary(0, false)).toBe("No computers paired");
  });
  it("does not turn a query failure into a zero count", () => {
    expect(connectSummary(undefined, false)).toBe("Loading computers…");
    expect(connectSummary(undefined, false, true)).toBe("Could not load computers");
  });
});
