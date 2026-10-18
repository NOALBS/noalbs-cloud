import { describe, expect, it } from "vitest";
import type { Notice } from "@/api/types";
import {
  fromLocalInput,
  parseDismissed,
  pruneDismissed,
  toLocalInput,
  visibleNotices,
} from "./notices";

const notice = (id: string, dismissible = true): Notice => ({
  id,
  message: id,
  severity: dismissible ? "info" : "critical",
  ends_at: null,
  dismissible,
});

describe("parseDismissed", () => {
  it("tolerates missing and malformed values", () => {
    expect(parseDismissed(null)).toEqual([]);
    expect(parseDismissed("nope")).toEqual([]);
    expect(parseDismissed('{"a":1}')).toEqual([]);
    expect(parseDismissed('["a",1,"b"]')).toEqual(["a", "b"]);
  });
});

describe("visibleNotices", () => {
  it("hides dismissed notices but never a non-dismissible one", () => {
    const notices = [notice("a"), notice("b"), notice("c", false)];
    expect(visibleNotices(notices, ["a", "c"]).map((n) => n.id)).toEqual(["b", "c"]);
  });
});

describe("pruneDismissed", () => {
  it("forgets notices that are over", () => {
    expect(pruneDismissed(["a", "gone"], [notice("a")])).toEqual(["a"]);
  });
});

describe("local date inputs", () => {
  it("round-trips to the minute", () => {
    const iso = new Date(2026, 9, 14, 9, 5).toISOString();
    expect(toLocalInput(iso)).toBe("2026-10-14T09:05");
    expect(fromLocalInput("2026-10-14T09:05")).toBe(iso);
  });

  it("treats empty and invalid as none", () => {
    expect(toLocalInput(null)).toBe("");
    expect(fromLocalInput("")).toBeNull();
    expect(fromLocalInput("nonsense")).toBeNull();
  });
});
