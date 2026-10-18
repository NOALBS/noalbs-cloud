import { describe, expect, it } from "vitest";
import { formatRelative } from "./time";

const now = new Date("2026-09-24T12:00:00Z");
const offset = (seconds: number) => new Date(now.getTime() + seconds * 1000);

describe("formatRelative", () => {
  it("formats future and past times", () => {
    expect(formatRelative(offset(6 * 24 * 3600), now)).toBe("in 6 days");
    expect(formatRelative(offset(24 * 3600), now)).toBe("tomorrow");
    expect(formatRelative(offset(-3 * 3600), now)).toBe("3 hours ago");
    expect(formatRelative(offset(10), now)).toBe("now");
  });
});
