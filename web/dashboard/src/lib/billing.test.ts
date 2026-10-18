import { describe, expect, it } from "vitest";
import type { Subscription } from "@/api/types";
import {
  countUnits,
  describeAddon,
  describeSubscription,
  formatPrice,
  missingCardReminder,
  prorate,
  quantityRange,
  showsAddon,
  unitsToCharge,
  yearlySaving,
} from "./billing";

const prices = [
  { interval: "month" as const, amount: 900, currency: "eur" },
  { interval: "year" as const, amount: 9000, currency: "eur" },
];

describe("formatPrice", () => {
  it("drops decimals only for whole amounts", () => {
    expect(formatPrice(900, "eur")).toBe("€9");
    expect(formatPrice(450, "usd")).toBe("$4.50");
  });
});

describe("yearlySaving", () => {
  it("compares a year with twelve months", () => {
    expect(yearlySaving(prices)).toBe(17);
  });

  it("is zero without both prices or without a saving", () => {
    expect(yearlySaving(prices.slice(0, 1))).toBe(0);
    expect(
      yearlySaving([
        prices[0],
        { interval: "year", amount: 12000, currency: "eur" },
      ] as typeof prices),
    ).toBe(0);
  });
});

describe("describeSubscription", () => {
  const base: Subscription = {
    status: "active",
    plan_code: "pro",
    interval: "month",
    current_period_end: "2026-10-24T12:00:00Z",
    cancel_at_period_end: false,
  };

  it("says when it renews", () => {
    expect(describeSubscription(base)).toEqual({
      text: "Monthly billing, renews on October 24, 2026.",
      warning: false,
    });
  });

  it("warns about a failed payment and a cancellation", () => {
    expect(describeSubscription({ ...base, status: "past_due" }).warning).toBe(true);
    expect(describeSubscription({ ...base, cancel_at_period_end: true }).text).toContain(
      "until October 24, 2026",
    );
  });
});

describe("add-ons", () => {
  const addon = (quantity: number, billed: number, held_until: string | null = null) => ({
    code: "ingest",
    quantity,
    billed,
    held_until,
  });

  it("charges only for units not already paid this period", () => {
    expect(unitsToCharge(undefined, 2)).toBe(2);
    expect(unitsToCharge(addon(1, 1), 3)).toBe(2);
    // Lowered from 3 to 1 this period: back to 3 is free, 4 charges one.
    expect(unitsToCharge(addon(3, 1, "2026-10-24T12:00:00Z"), 3)).toBe(0);
    expect(unitsToCharge(addon(3, 1, "2026-10-24T12:00:00Z"), 4)).toBe(1);
    expect(unitsToCharge(addon(2, 2), 1)).toBe(0);
  });

  it("prorates the rest of the period", () => {
    const start = "2026-09-01T00:00:00Z";
    const end = "2026-10-01T00:00:00Z";
    const halfway = Date.parse("2026-09-16T00:00:00Z");
    expect(prorate(500, 2, start, end, halfway)).toBe(500);
    expect(prorate(500, 0, start, end, halfway)).toBe(0);
    expect(prorate(500, 1, null, null)).toBe(500);
  });

  it("says what's kept until the period ends", () => {
    expect(describeAddon(addon(2, 2), "slot")).toBe("2 slots");
    expect(describeAddon(addon(3, 1, "2026-10-24T12:00:00Z"), "slot")).toBe(
      "3 slots until October 24, 2026, then 1",
    );
    expect(describeAddon(addon(1, 0, "2026-10-24T12:00:00Z"), "slot")).toBe(
      "1 slot until October 24, 2026, then none",
    );
    expect(countUnits(1, "slot")).toBe("1 slot");
  });
});

describe("add-on visibility", () => {
  const addon = (status: "hidden" | "coming_soon" | "unavailable" | "available") => ({
    code: "ingest",
    name: "Ingest",
    description: "",
    unit: "slot",
    max_quantity: 5,
    status,
    prices: [],
  });
  const owned = { code: "ingest", quantity: 2, billed: 2, held_until: null };

  it("hides hidden add-ons from workspaces that don't have them", () => {
    expect(showsAddon(addon("hidden"), undefined)).toBe(false);
    expect(showsAddon(addon("hidden"), owned)).toBe(true);
    expect(showsAddon(addon("coming_soon"), undefined)).toBe(true);
    expect(showsAddon(addon("unavailable"), undefined)).toBe(true);
  });

  it("only lets owners lower what isn't for sale", () => {
    expect(quantityRange(addon("available"), owned, true)).toEqual({ min: 0, max: 5 });
    expect(quantityRange(addon("unavailable"), owned, true)).toEqual({ min: 0, max: 2 });
    expect(quantityRange(addon("hidden"), undefined, false)).toEqual({ min: 1, max: 0 });
  });
});

describe("missingCardReminder", () => {
  const subscription = {
    current_period_end: "2026-10-28T10:00:00Z",
  } as Subscription;

  it("asks for a card before the renewal", () => {
    expect(
      missingCardReminder({ needs_payment_method: true, subscription, plan: { name: "Pro" } }),
    ).toBe(
      "There’s no card on file. Add one under Manage billing before October 28, 2026, or the Pro plan ends.",
    );
  });

  it("says nothing when a card is there", () => {
    expect(
      missingCardReminder({ needs_payment_method: false, subscription, plan: { name: "Pro" } }),
    ).toBeNull();
  });
});
