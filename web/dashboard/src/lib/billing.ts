import type {
  Addon,
  AddonVisibility,
  BillingInterval,
  PlanPrice,
  Subscription,
  WorkspaceAddon,
  WorkspacePlan,
} from "@/api/types";
import { currentLocale } from "@/lib/locale";

/** The one plan that's free by design (it can't be priced); other plans without a price aren't for sale yet. */
export const FREE_PLAN = "free";

/** "€9", "€90", "$4.50": whole amounts without decimals. */
export function formatPrice(amount: number, currency: string, locale = currentLocale()): string {
  const value = amount / 100;
  return new Intl.NumberFormat(locale, {
    style: "currency",
    currency: currency.toUpperCase(),
    minimumFractionDigits: Number.isInteger(value) ? 0 : 2,
    maximumFractionDigits: 2,
  }).format(value);
}

export function priceFor(prices: PlanPrice[], interval: BillingInterval): PlanPrice | undefined {
  return prices.find((price) => price.interval === interval);
}

/** How much a year costs less than twelve months, as a whole percentage (0 if not). */
export function yearlySaving(prices: PlanPrice[]): number {
  const month = priceFor(prices, "month");
  const year = priceFor(prices, "year");
  if (!month || !year || month.currency !== year.currency || month.amount <= 0) return 0;
  const saving = 1 - year.amount / (month.amount * 12);
  return saving > 0 ? Math.round(saving * 100) : 0;
}

function formatDay(date: string): string {
  return new Date(date).toLocaleDateString(currentLocale(), {
    day: "numeric",
    month: "long",
    year: "numeric",
  });
}

/** One line about the subscription, and whether it needs attention. */
export function describeSubscription(subscription: Subscription): {
  text: string;
  warning: boolean;
} {
  const end = subscription.current_period_end ? formatDay(subscription.current_period_end) : null;
  const period = subscription.interval === "year" ? "Yearly" : "Monthly";

  if (subscription.status === "past_due") {
    return {
      text: end
        ? `A payment failed. Update your card in Manage billing; the plan stays active while we retry (period ends ${end}).`
        : "A payment failed. Update your card in Manage billing; the plan stays active while we retry.",
      warning: true,
    };
  }
  if (subscription.cancel_at_period_end) {
    return {
      text: end
        ? `Canceled. The plan stays active until ${end}, then the workspace moves to Free.`
        : "Canceled. The plan stays active until the end of the period.",
      warning: true,
    };
  }
  return {
    text: end ? `${period} billing, renews on ${end}.` : `${period} billing.`,
    warning: false,
  };
}

/** "1 slot", "3 slots". */
export function countUnits(quantity: number, unit: string): string {
  return `${quantity} ${unit}${quantity === 1 ? "" : "s"}`;
}

/**
 * How many units raising to `target` charges now: only what isn't already paid
 * for this period (mirrors the API's `quantity_change`). Lowering charges nothing.
 */
export function unitsToCharge(addon: WorkspaceAddon | undefined, target: number): number {
  const paid = Math.max(addon?.billed ?? 0, addon?.quantity ?? 0);
  return Math.max(0, target - paid);
}

/**
 * About what Stripe charges now for `units` more at `amount` each, for the rest
 * of the period (it prorates by the second; this is an estimate for the dialog).
 */
export function prorate(
  amount: number,
  units: number,
  periodStart: string | null | undefined,
  periodEnd: string | null | undefined,
  now = Date.now(),
): number {
  if (!periodStart || !periodEnd || units <= 0) return amount * Math.max(units, 0);
  const start = Date.parse(periodStart);
  const end = Date.parse(periodEnd);
  if (end <= start) return amount * units;
  const left = Math.min(1, Math.max(0, (end - now) / (end - start)));
  return Math.round(amount * units * left);
}

/** "3 slots", or "3 slots until October 24, 2026, then 1" after lowering it. */
export function describeAddon(addon: WorkspaceAddon, unit: string): string {
  const now = countUnits(addon.quantity, unit);
  if (addon.billed < addon.quantity && addon.held_until) {
    const then = addon.billed === 0 ? "then none" : `then ${addon.billed}`;
    return `${now} until ${formatDay(addon.held_until)}, ${then}`;
  }
  return now;
}

export const ADDON_STATUS_LABELS: Record<AddonVisibility, string> = {
  hidden: "Hidden",
  coming_soon: "Coming soon",
  unavailable: "Currently unavailable",
  available: "Available",
};

/** Hidden add-ons still show to workspaces that have them, so they can lower them. */
export function showsAddon(addon: Addon, owned: WorkspaceAddon | undefined): boolean {
  return addon.status !== "hidden" || (owned?.quantity ?? 0) > 0;
}

/**
 * The quantities an owner may pick: anything up to the max while it's for sale;
 * otherwise only what they have or less (nobody can buy more).
 */
export function quantityRange(
  addon: Addon,
  owned: WorkspaceAddon | undefined,
  subscribed: boolean,
): { min: number; max: number } {
  const min = subscribed ? 0 : 1;
  if (addon.status === "available") return { min, max: addon.max_quantity };
  return { min, max: owned?.billed ?? 0 };
}

/**
 * The reminder after an ownership transfer removed the previous owner's cards:
 * the subscription renews at the end of the period and needs one by then.
 */
export function missingCardReminder(
  plan: Pick<WorkspacePlan, "needs_payment_method" | "subscription"> & { plan: { name: string } },
): string | null {
  if (!plan.needs_payment_method) return null;
  const end = plan.subscription?.current_period_end;
  return end
    ? `There’s no card on file. Add one under Manage billing before ${formatDay(end)}, or the ${plan.plan.name} plan ends.`
    : `There’s no card on file. Add one under Manage billing, or the ${plan.plan.name} plan ends with this period.`;
}
