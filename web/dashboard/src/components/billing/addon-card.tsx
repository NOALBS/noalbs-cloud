import { Minus, Plus } from "lucide-react";
import { useEffect, useState } from "react";
import type { Addon, BillingInterval, WorkspacePlan } from "@/api/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useBillingChange, useStripeRedirect } from "@/hooks/use-billing";
import {
  ADDON_STATUS_LABELS,
  countUnits,
  describeAddon,
  formatPrice,
  priceFor,
  prorate,
  quantityRange,
  unitsToCharge,
} from "@/lib/billing";
import { currentLocale } from "@/lib/locale";
import { ConfirmButton } from "./confirm-button";

/**
 * One add-on: its price per unit, what the workspace has, and a quantity to
 * buy or change. Without a subscription, buying goes through Checkout; with
 * one, more is charged now and fewer applies at the end of the period.
 */
export function AddonCard({
  addon,
  interval,
  workspacePlan,
}: {
  addon: Addon;
  interval: BillingInterval;
  workspacePlan: WorkspacePlan;
}) {
  const subscription = workspacePlan.subscription;
  const owned = workspacePlan.addons.find((item) => item.code === addon.code);
  // What continues after this period: the starting point for changes.
  const billed = owned?.billed ?? 0;
  const [quantity, setQuantity] = useState(Math.max(billed, subscription ? 0 : 1));
  useEffect(() => setQuantity(Math.max(billed, subscription ? 0 : 1)), [billed, subscription]);

  const checkout = useStripeRedirect();
  const change = useBillingChange(`${addon.name} updated`);
  // An existing subscription bills everything with its own period.
  const shownInterval = subscription?.interval ?? interval;
  const price = priceFor(addon.prices, shownInterval);
  const forSale = addon.status === "available" && price != null;
  const { min, max } = quantityRange(addon, owned, subscription != null);
  // Buying needs it for sale; lowering what you have is always allowed.
  const canChange =
    workspacePlan.billing_enabled &&
    workspacePlan.can_manage_billing &&
    (forSale || (subscription != null && billed > 0));
  const ending = subscription?.cancel_at_period_end ?? false;
  const label =
    addon.status === "available"
      ? forSale
        ? null
        : ADDON_STATUS_LABELS.coming_soon
      : addon.status === "hidden"
        ? "No longer offered"
        : ADDON_STATUS_LABELS[addon.status];

  const charged = unitsToCharge(owned, quantity);
  const periodEnd = subscription?.current_period_end
    ? new Date(subscription.current_period_end).toLocaleDateString(currentLocale(), {
        day: "numeric",
        month: "long",
      })
    : "the end of the period";

  return (
    <section className="flex flex-col gap-4 rounded-2xl border bg-card p-5.5">
      <div className="flex items-start justify-between gap-3">
        <div className="flex flex-col gap-1">
          <h3 className="text-lg font-semibold">{addon.name}</h3>
          <p className="text-sm text-muted-foreground">{addon.description}</p>
        </div>
        {label && (
          <Badge variant="secondary" className="shrink-0">
            {label}
          </Badge>
        )}
      </div>

      {price && (
        <p className="flex items-baseline gap-2 text-sm text-muted-foreground">
          <span className="font-mono text-xl font-semibold text-foreground tabular">
            {formatPrice(price.amount, price.currency)}
          </span>
          per {addon.unit} / {shownInterval}
        </p>
      )}

      {owned && owned.quantity > 0 && (
        <p className="text-sm">
          You have <strong>{describeAddon(owned, addon.unit)}</strong>.
          {!forSale && " You can keep or lower it, but not add more for now."}
        </p>
      )}

      {canChange && !ending && (
        <div className="mt-auto flex flex-wrap items-center gap-3">
          <div className="flex items-center rounded-xl border">
            <Button
              variant="ghost"
              size="icon"
              aria-label={`Fewer ${addon.unit}s`}
              disabled={quantity <= min}
              onClick={() => setQuantity(quantity - 1)}
            >
              <Minus />
            </Button>
            <span className="w-20 text-center font-mono text-sm tabular" aria-live="polite">
              {countUnits(quantity, addon.unit)}
            </span>
            <Button
              variant="ghost"
              size="icon"
              aria-label={`More ${addon.unit}s`}
              disabled={quantity >= max}
              onClick={() => setQuantity(quantity + 1)}
            >
              <Plus />
            </Button>
          </div>

          {!subscription ? (
            <Button
              onClick={() =>
                checkout.mutate({ interval, addons: [{ code: addon.code, quantity }] })
              }
              disabled={checkout.isPending}
            >
              {checkout.isPending ? "Opening checkout…" : `Buy ${countUnits(quantity, addon.unit)}`}
            </Button>
          ) : quantity !== billed ? (
            <ConfirmButton
              disabled={change.isPending}
              title={quantity > billed ? `Add ${addon.name}` : `Lower ${addon.name}`}
              confirmLabel={charged > 0 ? "Pay and update" : "Update"}
              onConfirm={() => change.mutate({ kind: "addon", code: addon.code, quantity })}
              description={
                charged > 0 && price ? (
                  <>
                    <span>
                      You’ll be charged about{" "}
                      <strong>
                        {formatPrice(
                          prorate(
                            price.amount,
                            charged,
                            subscription.current_period_start,
                            subscription.current_period_end,
                          ),
                          price.currency,
                        )}
                      </strong>{" "}
                      now for {countUnits(charged, addon.unit)} for the rest of this period, then{" "}
                      {formatPrice(price.amount * quantity, price.currency)} per {shownInterval}.
                    </span>
                    <span>If the card is declined, nothing changes.</span>
                  </>
                ) : quantity < billed ? (
                  <span>
                    You keep what you have until {periodEnd}. From then on you’re billed for{" "}
                    {quantity === 0 ? `no ${addon.unit}s` : countUnits(quantity, addon.unit)}.
                  </span>
                ) : (
                  <span>Already paid for this period, so nothing is charged now.</span>
                )
              }
            >
              {quantity === 0 ? `Remove ${addon.name}` : "Update"}
            </ConfirmButton>
          ) : null}
        </div>
      )}
      {canChange && ending && (
        <p className="mt-auto text-xs text-muted-foreground">
          Resume the subscription to change add-ons.
        </p>
      )}
    </section>
  );
}
