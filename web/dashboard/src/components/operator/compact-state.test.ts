import { describe, expect, it } from "vitest";
import type { SwitcherStatus } from "@/api/types";
import { compactAvailability } from "./compact-state";

const now = Date.parse("2026-11-10T12:00:00Z");
const status: SwitcherStatus = {
  updated_at: new Date(now).toISOString(),
  auto_switch: true,
  browser_source: true,
  obs_connected: true,
  streaming: true,
  recording: false,
  recording_paused: false,
  state: "live",
};
const operator = { role: "operator", staff: false } as const;
describe("compact live permission and freshness guards", () => {
  it("matches role/support permissions without granting management to operators", () => {
    expect(compactAvailability(operator, status, "open", false, now)).toMatchObject({
      control: true,
      manage: false,
    });
    expect(
      compactAvailability({ role: "owner", staff: false }, status, "open", false, now),
    ).toMatchObject({ control: true, manage: true });
    expect(
      compactAvailability(
        { role: null, organization: "member", staff: false },
        status,
        "open",
        false,
        now,
      ).control,
    ).toBe(true);
    expect(
      compactAvailability({ role: null, staff: true, support: "view" }, status, "open", false, now)
        .control,
    ).toBe(false);
    expect(
      compactAvailability(
        { role: null, staff: true, support: "control" },
        status,
        "open",
        false,
        now,
      ),
    ).toMatchObject({ control: true, manage: false });
  });
  it("disables controls for absent/disconnected/stale/paused/reconnecting status", () => {
    for (const current of [
      null,
      undefined,
      { ...status, obs_connected: false },
      { ...status, updated_at: new Date(now - 46000).toISOString() },
    ])
      expect(compactAvailability(operator, current, "open", false, now).control).toBe(false);
    expect(compactAvailability(operator, status, "open", true, now).control).toBe(false);
    expect(compactAvailability(operator, status, "connecting", false, now).control).toBe(false);
    expect(compactAvailability(operator, status, "reconnecting", false, now).control).toBe(false);
  });
});
